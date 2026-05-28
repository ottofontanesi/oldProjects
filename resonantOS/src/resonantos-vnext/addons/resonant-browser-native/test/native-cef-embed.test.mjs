import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { existsSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { promisify } from "node:util";

const execFileAsync = promisify(execFile);
const addonRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(addonRoot, "..", "..");
const bridgeDylib = path.join(addonRoot, "build", "libResonantBrowserNativeBridgeShared.dylib");
const cefFramework = path.join(
  addonRoot,
  "vendor",
  "cef",
  "cef_binary_147.0.10+gd58e84d+chromium-147.0.7727.118_macosarm64",
  "Release",
  "Chromium Embedded Framework.framework",
);
const helper = path.join(
  addonRoot,
  "build",
  "ResonantBrowserNativeHost.app",
  "Contents",
  "Frameworks",
  "ResonantBrowserNativeHost Helper.app",
  "Contents",
  "MacOS",
  "ResonantBrowserNativeHost Helper",
);

test(
  "native CEF bridge embeds into a real macOS NSView and loads a page",
  {
    skip:
      process.platform !== "darwin" || !existsSync(bridgeDylib) || !existsSync(cefFramework) || !existsSync(helper)
        ? "macOS native bridge, CEF framework, and helper app are required for embedded smoke."
        : false,
  },
  async () => {
    const harnessSource = path.join(tmpdir(), "resonant_browser_embed_harness.mm");
    const harnessBinary = path.join(tmpdir(), "resonant_browser_embed_harness");
    writeFileSync(
      harnessSource,
      `
#import <Cocoa/Cocoa.h>
#include <chrono>
#include <iostream>
#include <string>

extern "C" const char* resonant_browser_native_prepare_macos_application_json(void);
extern "C" const char* resonant_browser_native_initialize_json(const char*, const char*, const char*);
extern "C" const char* resonant_browser_native_attach_macos_ns_view_json(void*, int, int, int, int, const char*);
extern "C" const char* resonant_browser_native_status_json(void);
extern "C" const char* resonant_browser_native_close_json(void);

int main() {
  @autoreleasepool {
    std::cout << resonant_browser_native_prepare_macos_application_json() << std::endl;
    std::cout << resonant_browser_native_initialize_json("${cefFramework}", "${helper}", "${path.join(
        tmpdir(),
        "resonantos-native-browser-harness-cache",
      )}") << std::endl;

    NSRect frame = NSMakeRect(0, 0, 900, 700);
    NSWindow* window = [[NSWindow alloc] initWithContentRect:frame
                                                   styleMask:(NSWindowStyleMaskTitled | NSWindowStyleMaskClosable | NSWindowStyleMaskResizable)
                                                     backing:NSBackingStoreBuffered
                                                       defer:NO];
    [window setTitle:@"Resonant Browser Native Harness"];
    [window makeKeyAndOrderFront:nil];
    NSView* view = [window contentView];
    std::cout << resonant_browser_native_attach_macos_ns_view_json((__bridge void*)view, 0, 0, 900, 700, "https://example.com") << std::endl;

    auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(15);
    std::string last;
    while (std::chrono::steady_clock::now() < deadline) {
      @autoreleasepool {
        NSDate* until = [NSDate dateWithTimeIntervalSinceNow:0.05];
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:until];
      }
      last = resonant_browser_native_status_json();
      if (last.find("browser.native.embedded.load_end") != std::string::npos && last.find("httpStatus") != std::string::npos && last.find(":200") != std::string::npos) {
        std::cout << last << std::endl;
        std::cout << resonant_browser_native_close_json() << std::endl;
        return 0;
      }
    }
    std::cerr << "Timed out waiting for embedded CEF load. Last status: " << last << std::endl;
    std::cout << resonant_browser_native_close_json() << std::endl;
    return 2;
  }
}
`,
    );

    await execFileAsync(
      "clang++",
      [
        "-std=c++20",
        "-fobjc-arc",
        "-framework",
        "Cocoa",
        harnessSource,
        `-L${path.dirname(bridgeDylib)}`,
        "-lResonantBrowserNativeBridgeShared",
        `-Wl,-rpath,${path.dirname(bridgeDylib)}`,
        "-o",
        harnessBinary,
      ],
      { cwd: repoRoot, timeout: 20000, maxBuffer: 1024 * 1024 },
    );

    const { stdout } = await execFileAsync(harnessBinary, [], {
      cwd: repoRoot,
      timeout: 25000,
      maxBuffer: 1024 * 1024 * 4,
    });
    assert.match(stdout, /"stage":"prepare-application"/);
    assert.match(stdout, /"detail":"CEF initialized in the ResonantOS process\."/);
    assert.match(stdout, /"stage":"attach-view"/);
    assert.match(stdout, /"event":"browser\.native\.embedded\.load_end"/);
    assert.match(stdout, /"httpStatus":200/);
    assert.match(stdout, /"url":"https:\/\/example\.com\//);
  },
);
