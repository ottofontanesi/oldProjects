// Intent citation: docs/architecture/ADR-025-native-embedded-browser-host.md
//
// Native in-process CEF bridge for macOS. This is the product direction for
// Resonant Browser: CEF attaches to the ResonantOS-owned center workspace view
// through a narrow C ABI instead of creating an external window or WebView.

#import <Cocoa/Cocoa.h>

#include <algorithm>
#include <mutex>
#include <filesystem>
#include <sstream>
#include <string>

#include "include/cef_app.h"
#include "include/cef_browser.h"
#include "include/cef_client.h"
#include "include/cef_command_line.h"
#include "include/cef_request_context.h"
#include "include/cef_task.h"
#include "include/cef_application_mac.h"
#include "include/wrapper/cef_helpers.h"
#include "include/wrapper/cef_library_loader.h"
#include "resonant_browser_native_bridge.h"

@interface ResonantBrowserBridgeApplication : NSApplication <CefAppProtocol> {
 @private
  BOOL handlingSendEvent_;
}
@end

@implementation ResonantBrowserBridgeApplication
- (BOOL)isHandlingSendEvent {
  return handlingSendEvent_;
}

- (void)setHandlingSendEvent:(BOOL)handlingSendEvent {
  handlingSendEvent_ = handlingSendEvent;
}

- (void)sendEvent:(NSEvent*)event {
  CefScopedSendingEvent sendingEventScoper;
  [super sendEvent:event];
}

- (void)terminate:(id)sender {
  // Embedded bridge lifecycle is owned by the Tauri host. AppKit can still
  // deliver terminate: to this CefAppProtocol-aware NSApplication class; do
  // not route that through CEF or super here because the bridge is not running
  // a standalone CEF application message loop.
  (void)sender;
}
@end

namespace {

std::mutex g_state_mutex;
std::string g_last_json = R"json({"status":"not-started","engineCandidate":"cef-chrome-runtime"})json";
bool g_prepared_application = false;
bool g_initialized = false;
CefScopedLibraryLoader* g_library_loader = nullptr;
CefRefPtr<CefBrowser> g_browser;
NSTimer* g_message_pump_timer = nil;
NSView* g_parent_view = nil;

std::string EscapeJson(const std::string& value) {
  std::ostringstream escaped;
  for (char character : value) {
    switch (character) {
      case '\\':
        escaped << "\\\\";
        break;
      case '"':
        escaped << "\\\"";
        break;
      case '\n':
        escaped << "\\n";
        break;
      case '\r':
        escaped << "\\r";
        break;
      case '\t':
        escaped << "\\t";
        break;
      default:
        escaped << character;
        break;
    }
  }
  return escaped.str();
}

const char* StoreJson(const std::string& json) {
  std::lock_guard<std::mutex> lock(g_state_mutex);
  g_last_json = json;
  return g_last_json.c_str();
}

const char* JsonStatus(const std::string& status, const std::string& detail) {
  return StoreJson("{\"status\":\"" + EscapeJson(status) + "\",\"detail\":\"" + EscapeJson(detail) +
                   "\",\"engineCandidate\":\"cef-chrome-runtime\"}");
}

std::string MainBundlePathFromHelperPath(const char* helper_executable_path) {
  if (!helper_executable_path || helper_executable_path[0] == '\0') {
    return "";
  }
  std::filesystem::path helper_path(helper_executable_path);
  const std::string marker = ".app/Contents/Frameworks/";
  const std::string path_text = helper_path.string();
  const std::size_t marker_index = path_text.find(marker);
  if (marker_index == std::string::npos) {
    return "";
  }
  return path_text.substr(0, marker_index + 4);
}

class BridgeClient final : public CefClient, public CefLifeSpanHandler, public CefLoadHandler {
 public:
  BridgeClient() = default;

  CefRefPtr<CefLifeSpanHandler> GetLifeSpanHandler() override { return this; }
  CefRefPtr<CefLoadHandler> GetLoadHandler() override { return this; }

  void OnAfterCreated(CefRefPtr<CefBrowser> browser) override {
    CEF_REQUIRE_UI_THREAD();
    g_browser = browser;
    StoreJson("{\"status\":\"attached\",\"event\":\"browser.native.embedded.created\"}");
  }

  void OnBeforeClose(CefRefPtr<CefBrowser> browser) override {
    CEF_REQUIRE_UI_THREAD();
    if (g_browser && g_browser->IsSame(browser)) {
      g_browser = nullptr;
    }
    StoreJson("{\"status\":\"closed\",\"event\":\"browser.native.embedded.closed\"}");
  }

  void OnLoadEnd(CefRefPtr<CefBrowser> browser,
                 CefRefPtr<CefFrame> frame,
                 int http_status_code) override {
    CEF_REQUIRE_UI_THREAD();
    if (frame && frame->IsMain()) {
      StoreJson("{\"status\":\"loaded\",\"event\":\"browser.native.embedded.load_end\",\"httpStatus\":" +
                std::to_string(http_status_code) + ",\"url\":\"" +
                EscapeJson(frame->GetURL().ToString()) + "\"}");
    }
  }

 private:
  IMPLEMENT_REFCOUNTING(BridgeClient);
  DISALLOW_COPY_AND_ASSIGN(BridgeClient);
};

class BridgeApp final : public CefApp, public CefBrowserProcessHandler {
 public:
  BridgeApp() = default;

  CefRefPtr<CefBrowserProcessHandler> GetBrowserProcessHandler() override { return this; }

  void OnBeforeCommandLineProcessing(const CefString& process_type,
                                     CefRefPtr<CefCommandLine> command_line) override {
    if (process_type.empty()) {
      command_line->AppendSwitch("enable-chrome-runtime");
      command_line->AppendSwitch("disable-features=GlobalMediaControls");
      command_line->AppendSwitch("disable-gpu");
      command_line->AppendSwitch("disable-gpu-compositing");
      command_line->AppendSwitch("use-mock-keychain");
      command_line->AppendSwitchWithValue("password-store", "basic");
      command_line->AppendSwitchWithValue("remote-debugging-port", "0");
    }
  }

 private:
  IMPLEMENT_REFCOUNTING(BridgeApp);
  DISALLOW_COPY_AND_ASSIGN(BridgeApp);
};

CefRefPtr<BridgeClient> g_client;
CefRefPtr<BridgeApp> g_app;

CefRect CefChildRectFromDomBounds(NSView* parent_view, int x, int y, int width, int height) {
  // Intent citation: docs/architecture/ADR-025-native-embedded-browser-host.md
  // ResonantOS measures the Browser mount in DOM/top-left coordinates. macOS
  // child views are positioned in the parent NSView coordinate space, which is
  // bottom-left for the Tauri content view. Convert here so the native Chromium
  // surface cannot drift upward and cover the Browser menu/address bars.
  const NSRect parent_bounds = [parent_view bounds];
  const int parent_height = static_cast<int>(NSHeight(parent_bounds));
  const int converted_y = std::max(0, parent_height - y - height);
  return CefRect(x, converted_y, width, height);
}

}  // namespace

extern "C" const char* resonant_browser_native_prepare_macos_application_json(void) {
  @autoreleasepool {
    if (NSApp == nil) {
      [ResonantBrowserBridgeApplication sharedApplication];
    }

    const bool compatible = [NSApp conformsToProtocol:@protocol(CefAppProtocol)];
    g_prepared_application = compatible;
    if (!compatible) {
      const char* class_name = object_getClassName(NSApp);
      return StoreJson("{\"status\":\"blocked\",\"stage\":\"prepare-application\",\"detail\":\"Existing "
                       "NSApplication is not CefAppProtocol-compatible\",\"nsApplicationClass\":\"" +
                       EscapeJson(class_name ? class_name : "unknown") + "\"}");
    }

    return StoreJson("{\"status\":\"ready\",\"stage\":\"prepare-application\",\"nsApplicationClass\":\"" +
                     EscapeJson(object_getClassName(NSApp)) + "\"}");
  }
}

extern "C" const char* resonant_browser_native_initialize_json(const char* framework_dir_path,
                                                               const char* helper_executable_path,
                                                               const char* cache_dir_path) {
  if (!g_prepared_application) {
    return JsonStatus("blocked", "Call resonant_browser_native_prepare_macos_application_json before initializing CEF.");
  }
  if (g_initialized) {
    return JsonStatus("ready", "CEF is already initialized.");
  }

  if (!g_library_loader) {
    bool loaded = false;
    if (framework_dir_path && framework_dir_path[0] != '\0') {
      std::string framework_binary = std::string(framework_dir_path) + "/Chromium Embedded Framework";
      loaded = cef_load_library(framework_binary.c_str()) != 0;
    }
    if (!loaded) {
      g_library_loader = new CefScopedLibraryLoader();
      loaded = g_library_loader->LoadInMain();
    }
    if (!loaded) {
      return JsonStatus("blocked", "CEF framework failed to load in the main process.");
    }
  }

  CefMainArgs main_args;
  g_app = new BridgeApp();
  CefSettings settings;
  settings.no_sandbox = true;
  settings.external_message_pump = true;
  if (framework_dir_path && framework_dir_path[0] != '\0') {
    CefString(&settings.framework_dir_path) = framework_dir_path;
  }
  if (helper_executable_path && helper_executable_path[0] != '\0') {
    CefString(&settings.browser_subprocess_path) = helper_executable_path;
    const std::string main_bundle_path = MainBundlePathFromHelperPath(helper_executable_path);
    if (!main_bundle_path.empty()) {
      CefString(&settings.main_bundle_path) = main_bundle_path;
    }
  }
  if (cache_dir_path && cache_dir_path[0] != '\0') {
    CefString(&settings.cache_path) = cache_dir_path;
    CefString(&settings.root_cache_path) = cache_dir_path;
  }

  if (!CefInitialize(main_args, settings, g_app, nullptr)) {
    return JsonStatus("blocked", "CefInitialize returned false.");
  }

  g_initialized = true;
  if (!g_message_pump_timer) {
    g_message_pump_timer = [NSTimer scheduledTimerWithTimeInterval:0.01
                                                           repeats:YES
                                                             block:^(__unused NSTimer* timer) {
                                                               CefDoMessageLoopWork();
                                                             }];
  }
  return JsonStatus("ready", "CEF initialized in the ResonantOS process.");
}

extern "C" const char* resonant_browser_native_attach_macos_ns_view_json(void* parent_ns_view,
                                                                         int x,
                                                                         int y,
                                                                         int width,
                                                                         int height,
                                                                         const char* url) {
  if (!g_initialized) {
    return JsonStatus("blocked", "CEF is not initialized.");
  }
  if (!parent_ns_view) {
    return JsonStatus("blocked", "Parent NSView pointer is missing.");
  }
  if (width <= 0 || height <= 0) {
    return JsonStatus("blocked", "Browser bounds must be positive.");
  }

  g_client = new BridgeClient();
  CefWindowInfo window_info;
  window_info.runtime_style = CEF_RUNTIME_STYLE_CHROME;
  NSView* parent_view = static_cast<NSView*>(parent_ns_view);
  g_parent_view = parent_view;
  window_info.SetAsChild(static_cast<CefWindowHandle>(parent_ns_view), CefChildRectFromDomBounds(parent_view, x, y, width, height));
  CefBrowserSettings browser_settings;
  const std::string target_url = url && url[0] != '\0' ? url : "https://resonantos.com";
  const bool created = CefBrowserHost::CreateBrowser(
      window_info, g_client, target_url, browser_settings, nullptr, CefRequestContext::GetGlobalContext());
  if (!created) {
    return JsonStatus("blocked", "CEF failed to create an embedded child browser.");
  }

  return StoreJson("{\"status\":\"attaching\",\"stage\":\"attach-view\",\"url\":\"" +
                   EscapeJson(target_url) + "\"}");
}

extern "C" const char* resonant_browser_native_resize_json(int x, int y, int width, int height) {
  if (!g_browser) {
    return JsonStatus("blocked", "No embedded browser exists.");
  }
  CefWindowHandle window_handle = g_browser->GetHost()->GetWindowHandle();
  NSView* browser_view = static_cast<NSView*>(window_handle);
  if (!browser_view) {
    return JsonStatus("blocked", "Embedded browser view handle is unavailable.");
  }
  if (!g_parent_view) {
    return JsonStatus("blocked", "Embedded browser parent view is unavailable.");
  }
  const CefRect converted_bounds = CefChildRectFromDomBounds(g_parent_view, x, y, width, height);
  [browser_view setFrame:NSMakeRect(converted_bounds.x, converted_bounds.y, converted_bounds.width, converted_bounds.height)];
  return StoreJson("{\"status\":\"resized\",\"stage\":\"resize-view\"}");
}

extern "C" const char* resonant_browser_native_navigate_json(const char* url) {
  if (!g_browser) {
    return JsonStatus("blocked", "No embedded browser exists.");
  }
  if (!url || url[0] == '\0') {
    return JsonStatus("blocked", "Navigation URL is empty.");
  }
  g_browser->GetMainFrame()->LoadURL(url);
  return StoreJson("{\"status\":\"navigating\",\"stage\":\"navigate\",\"url\":\"" + EscapeJson(url) + "\"}");
}

extern "C" const char* resonant_browser_native_close_json(void) {
  if (g_browser) {
    g_browser->GetHost()->CloseBrowser(true);
    return StoreJson("{\"status\":\"closing\",\"stage\":\"close\"}");
  }
  return StoreJson("{\"status\":\"closed\",\"stage\":\"close\"}");
}

extern "C" const char* resonant_browser_native_status_json(void) {
  std::lock_guard<std::mutex> lock(g_state_mutex);
  return g_last_json.c_str();
}
