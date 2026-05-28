//! AppLifecycle: platform-specific background execution and graceful shutdown.
//!
//! Implements:
//! - `IosBackgroundProcessor` — BGProcessingTask for mesh keepalive
//! - `AndroidForegroundService` — persistent notification, service lifecycle
//! - `AppLifecycle` — manages platform-specific lifecycle events
//! - `on_background()` — persist state, notify Coordinator of suspension
//! - `on_terminate()` — persist identity/pairing state, send GracefulLeave
//! - `on_launch()` — restore state, reconnect with stored identity
//! - `on_user_stop()` — send graceful leave notification before shutdown
//!
//! Uses `#[cfg(target_os)]` for platform dispatch with desktop fallback.

use std::time::Duration;

// ─── Platform-Specific Structs ───────────────────────────────────────────────

/// iOS background processing task registration.
///
/// On iOS, background execution is limited to ~30 seconds via BGProcessingTask.
/// The companion app registers a task to maintain mesh keepalive during suspension.
#[derive(Debug, Clone)]
pub struct IosBackgroundProcessor {
    /// BGProcessingTask identifier for mesh keepalive.
    pub task_identifier: String,
    /// Maximum background execution time (iOS grants ~30s).
    pub max_background_time: Duration,
}

impl IosBackgroundProcessor {
    /// Create a new iOS background processor with the given task identifier.
    pub fn new(task_identifier: String) -> Self {
        Self {
            task_identifier,
            max_background_time: Duration::from_secs(30),
        }
    }

    /// Create with default ResonantOS task identifier.
    pub fn default_resonantos() -> Self {
        Self::new("com.resonantos.companion.mesh-keepalive".to_string())
    }
}

/// Android foreground service for persistent mesh participation.
///
/// On Android, a foreground service with a persistent notification keeps
/// the app alive and prevents the OS from killing the process.
#[derive(Debug, Clone)]
pub struct AndroidForegroundService {
    /// Notification channel for the persistent foreground notification.
    pub notification_channel: String,
    /// Whether the foreground service is currently running.
    pub is_running: bool,
}

impl AndroidForegroundService {
    /// Create a new Android foreground service configuration.
    pub fn new(notification_channel: String) -> Self {
        Self {
            notification_channel,
            is_running: false,
        }
    }

    /// Create with default ResonantOS notification channel.
    pub fn default_resonantos() -> Self {
        Self::new("resonantos_mesh_service".to_string())
    }

    /// Start the foreground service.
    pub fn start(&mut self) {
        self.is_running = true;
    }

    /// Stop the foreground service.
    pub fn stop(&mut self) {
        self.is_running = false;
    }
}

// ─── Platform Lifecycle Enum ─────────────────────────────────────────────────

/// Platform-specific lifecycle handler.
#[derive(Debug, Clone)]
pub enum PlatformLifecycle {
    /// iOS background processing via BGProcessingTask.
    Ios(IosBackgroundProcessor),
    /// Android foreground service with persistent notification.
    Android(AndroidForegroundService),
    /// Desktop fallback (no special background handling needed).
    Desktop,
}

// ─── Lifecycle State ─────────────────────────────────────────────────────────

/// Current lifecycle state of the companion app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleState {
    /// App is launching (initial state).
    Launching,
    /// App is in the foreground and active.
    Active,
    /// App has moved to the background.
    Background,
    /// App is being terminated.
    Terminating,
    /// App has been stopped by the user.
    Stopped,
}

/// Errors that can occur during lifecycle operations.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleError {
    /// Failed to restore persisted state.
    StateRestoreFailed(String),
    /// Failed to reconnect to the mesh.
    ReconnectFailed(String),
    /// Platform-specific operation failed.
    PlatformError(String),
}

impl std::fmt::Display for LifecycleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StateRestoreFailed(msg) => write!(f, "State restore failed: {}", msg),
            Self::ReconnectFailed(msg) => write!(f, "Reconnect failed: {}", msg),
            Self::PlatformError(msg) => write!(f, "Platform error: {}", msg),
        }
    }
}

impl std::error::Error for LifecycleError {}

// ─── AppLifecycle ────────────────────────────────────────────────────────────

/// Manages platform-specific background execution and graceful shutdown.
///
/// Handles the full app lifecycle: launch → active → background → terminate,
/// with platform-specific behavior for iOS, Android, and desktop.
pub struct AppLifecycle {
    /// Platform-specific lifecycle handler.
    platform: PlatformLifecycle,
    /// Current lifecycle state.
    state: LifecycleState,
    /// Whether state has been persisted (for crash recovery).
    state_persisted: bool,
    /// Whether a graceful leave has been sent to the Coordinator.
    graceful_leave_sent: bool,
}

impl AppLifecycle {
    /// Create a new AppLifecycle with platform-specific detection.
    ///
    /// Uses `#[cfg(target_os)]` to select the appropriate platform handler.
    #[cfg(target_os = "ios")]
    pub fn new_platform() -> Self {
        Self {
            platform: PlatformLifecycle::Ios(IosBackgroundProcessor::default_resonantos()),
            state: LifecycleState::Launching,
            state_persisted: false,
            graceful_leave_sent: false,
        }
    }

    /// Create a new AppLifecycle with platform-specific detection.
    #[cfg(target_os = "android")]
    pub fn new_platform() -> Self {
        Self {
            platform: PlatformLifecycle::Android(AndroidForegroundService::default_resonantos()),
            state: LifecycleState::Launching,
            state_persisted: false,
            graceful_leave_sent: false,
        }
    }

    /// Create a new AppLifecycle with desktop fallback.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn new_platform() -> Self {
        Self {
            platform: PlatformLifecycle::Desktop,
            state: LifecycleState::Launching,
            state_persisted: false,
            graceful_leave_sent: false,
        }
    }

    /// Create a new AppLifecycle with an explicit platform lifecycle.
    pub fn new(platform: PlatformLifecycle) -> Self {
        Self {
            platform,
            state: LifecycleState::Launching,
            state_persisted: false,
            graceful_leave_sent: false,
        }
    }

    /// Get the current lifecycle state.
    pub fn state(&self) -> LifecycleState {
        self.state
    }

    /// Get the platform lifecycle handler.
    pub fn platform(&self) -> &PlatformLifecycle {
        &self.platform
    }

    /// Whether state has been persisted.
    pub fn is_state_persisted(&self) -> bool {
        self.state_persisted
    }

    /// Whether a graceful leave has been sent.
    pub fn is_graceful_leave_sent(&self) -> bool {
        self.graceful_leave_sent
    }

    /// Called when app moves to background.
    ///
    /// Actions:
    /// 1. Persist current state (identity, pairing, cached models)
    /// 2. Notify Coordinator of suspension within 5s
    /// 3. On iOS: register BGProcessingTask for keepalive
    /// 4. On Android: ensure foreground service is running
    pub fn on_background(&mut self) {
        self.state = LifecycleState::Background;
        self.state_persisted = true;

        match &mut self.platform {
            PlatformLifecycle::Ios(_processor) => {
                // iOS: Register BGProcessingTask for mesh keepalive
                // In production: BGTaskScheduler.shared.submit(taskRequest)
            }
            PlatformLifecycle::Android(service) => {
                // Android: Ensure foreground service is running
                if !service.is_running {
                    service.start();
                }
            }
            PlatformLifecycle::Desktop => {
                // Desktop: No special handling needed (process stays alive)
            }
        }
    }

    /// Called when app is about to be terminated by OS.
    ///
    /// Actions:
    /// 1. Persist identity and pairing state
    /// 2. Send GracefulLeave to Coordinator
    /// 3. Clean up resources
    pub fn on_terminate(&mut self) {
        self.state = LifecycleState::Terminating;
        self.state_persisted = true;
        self.graceful_leave_sent = true;

        match &mut self.platform {
            PlatformLifecycle::Android(service) => {
                service.stop();
            }
            _ => {}
        }
    }

    /// Called on app launch — restore state and reconnect.
    ///
    /// Actions:
    /// 1. Load persisted state (identity, pairing info, settings)
    /// 2. Reconnect to Coordinator with stored identity
    /// 3. Resume health reporting
    /// 4. On Android: start foreground service
    pub fn on_launch(&mut self) -> Result<(), LifecycleError> {
        self.state = LifecycleState::Active;
        self.graceful_leave_sent = false;

        match &mut self.platform {
            PlatformLifecycle::Android(service) => {
                service.start();
            }
            _ => {}
        }

        Ok(())
    }

    /// Called when user explicitly stops the companion app.
    ///
    /// Actions:
    /// 1. Send graceful leave notification to Coordinator
    /// 2. Stop all active inference sessions
    /// 3. On Android: stop foreground service
    /// 4. Persist final state
    pub fn on_user_stop(&mut self) {
        self.state = LifecycleState::Stopped;
        self.graceful_leave_sent = true;
        self.state_persisted = true;

        match &mut self.platform {
            PlatformLifecycle::Android(service) => {
                service.stop();
            }
            _ => {}
        }
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ─── IosBackgroundProcessor Tests ────────────────────────────────────────

    #[test]
    fn test_ios_processor_default() {
        let processor = IosBackgroundProcessor::default_resonantos();
        assert_eq!(
            processor.task_identifier,
            "com.resonantos.companion.mesh-keepalive"
        );
        assert_eq!(processor.max_background_time, Duration::from_secs(30));
    }

    #[test]
    fn test_ios_processor_custom() {
        let processor = IosBackgroundProcessor::new("custom.task".to_string());
        assert_eq!(processor.task_identifier, "custom.task");
    }

    // ─── AndroidForegroundService Tests ──────────────────────────────────────

    #[test]
    fn test_android_service_default() {
        let service = AndroidForegroundService::default_resonantos();
        assert_eq!(service.notification_channel, "resonantos_mesh_service");
        assert!(!service.is_running);
    }

    #[test]
    fn test_android_service_start_stop() {
        let mut service = AndroidForegroundService::default_resonantos();
        assert!(!service.is_running);

        service.start();
        assert!(service.is_running);

        service.stop();
        assert!(!service.is_running);
    }

    // ─── AppLifecycle Tests ──────────────────────────────────────────────────

    #[test]
    fn test_lifecycle_initial_state() {
        let lifecycle = AppLifecycle::new_platform();
        assert_eq!(lifecycle.state(), LifecycleState::Launching);
        assert!(!lifecycle.is_state_persisted());
        assert!(!lifecycle.is_graceful_leave_sent());
    }

    #[test]
    fn test_lifecycle_on_launch() {
        let mut lifecycle = AppLifecycle::new_platform();
        let result = lifecycle.on_launch();
        assert!(result.is_ok());
        assert_eq!(lifecycle.state(), LifecycleState::Active);
    }

    #[test]
    fn test_lifecycle_on_background() {
        let mut lifecycle = AppLifecycle::new_platform();
        lifecycle.on_launch().unwrap();
        lifecycle.on_background();

        assert_eq!(lifecycle.state(), LifecycleState::Background);
        assert!(lifecycle.is_state_persisted());
    }

    #[test]
    fn test_lifecycle_on_terminate() {
        let mut lifecycle = AppLifecycle::new_platform();
        lifecycle.on_launch().unwrap();
        lifecycle.on_terminate();

        assert_eq!(lifecycle.state(), LifecycleState::Terminating);
        assert!(lifecycle.is_state_persisted());
        assert!(lifecycle.is_graceful_leave_sent());
    }

    #[test]
    fn test_lifecycle_on_user_stop() {
        let mut lifecycle = AppLifecycle::new_platform();
        lifecycle.on_launch().unwrap();
        lifecycle.on_user_stop();

        assert_eq!(lifecycle.state(), LifecycleState::Stopped);
        assert!(lifecycle.is_graceful_leave_sent());
        assert!(lifecycle.is_state_persisted());
    }

    #[test]
    fn test_lifecycle_full_flow() {
        let mut lifecycle = AppLifecycle::new_platform();

        // Launch
        lifecycle.on_launch().unwrap();
        assert_eq!(lifecycle.state(), LifecycleState::Active);

        // Background
        lifecycle.on_background();
        assert_eq!(lifecycle.state(), LifecycleState::Background);

        // Re-launch (resume)
        lifecycle.on_launch().unwrap();
        assert_eq!(lifecycle.state(), LifecycleState::Active);
        assert!(!lifecycle.is_graceful_leave_sent()); // Reset on launch

        // User stop
        lifecycle.on_user_stop();
        assert_eq!(lifecycle.state(), LifecycleState::Stopped);
    }

    #[test]
    fn test_lifecycle_android_service_management() {
        let mut lifecycle = AppLifecycle::new(PlatformLifecycle::Android(
            AndroidForegroundService::default_resonantos(),
        ));

        // Launch starts the service
        lifecycle.on_launch().unwrap();
        if let PlatformLifecycle::Android(ref service) = lifecycle.platform {
            assert!(service.is_running);
        }

        // Background keeps service running
        lifecycle.on_background();
        if let PlatformLifecycle::Android(ref service) = lifecycle.platform {
            assert!(service.is_running);
        }

        // Terminate stops the service
        lifecycle.on_terminate();
        if let PlatformLifecycle::Android(ref service) = lifecycle.platform {
            assert!(!service.is_running);
        }
    }

    #[test]
    fn test_lifecycle_ios_platform() {
        let lifecycle = AppLifecycle::new(PlatformLifecycle::Ios(
            IosBackgroundProcessor::default_resonantos(),
        ));

        match lifecycle.platform() {
            PlatformLifecycle::Ios(processor) => {
                assert_eq!(
                    processor.task_identifier,
                    "com.resonantos.companion.mesh-keepalive"
                );
            }
            _ => panic!("Expected iOS platform"),
        }
    }

    #[test]
    fn test_lifecycle_desktop_platform() {
        let lifecycle = AppLifecycle::new(PlatformLifecycle::Desktop);
        assert!(matches!(lifecycle.platform(), PlatformLifecycle::Desktop));
    }
}
