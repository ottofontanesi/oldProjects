// resonantos-node — headless compute node daemon.
//
// Usage: resonantos-node [--join] [--peer addr] [--low-power] [--config path]
//
// Joins the ResonantOS mesh as a compute node without GUI.
// Managed remotely from the desktop app's dashboard.

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Parse CLI
    let mut config = resonantos_vnext::daemon::config::NodeConfig::default();
    let mut overrides = resonantos_vnext::daemon::config::CliOverrides::default();

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--peer" => {
                i += 1;
                if i < args.len() {
                    overrides.peers.push(args[i].clone());
                }
            }
            "--port" => {
                i += 1;
                if i < args.len() {
                    overrides.port = args[i].parse().ok();
                }
            }
            "--config" => {
                i += 1;
                if i < args.len() {
                    overrides.config_path = Some(std::path::PathBuf::from(&args[i]));
                }
            }
            "--models-dir" => {
                i += 1;
                if i < args.len() {
                    overrides.models_dir = Some(std::path::PathBuf::from(&args[i]));
                }
            }
            "--low-power" => overrides.low_power = true,
            "--daemon" => overrides.daemon_mode = true,
            "--status" => overrides.status_query = true,
            "--shutdown" => overrides.shutdown_request = true,
            "--join" | _ => {} // Default behavior is join
        }
        i += 1;
    }

    // Load config file if specified
    if let Some(ref path) = overrides.config_path {
        config = resonantos_vnext::daemon::config::NodeConfig::load(path);
    }
    config.apply_overrides(&overrides);

    // Handle --status (query running daemon)
    if overrides.status_query {
        eprintln!("Querying daemon at 127.0.0.1:{}...", config.daemon.api_port);
        // In production: HTTP GET to localhost:api_port/status
        eprintln!("(Not implemented yet — daemon must expose HTTP API)");
        std::process::exit(0);
    }

    // Handle --shutdown (send shutdown to running daemon)
    if overrides.shutdown_request {
        eprintln!("Sending shutdown to daemon at 127.0.0.1:{}...", config.daemon.api_port);
        // In production: HTTP POST to localhost:api_port/shutdown
        eprintln!("(Not implemented yet — daemon must expose HTTP API)");
        std::process::exit(0);
    }

    // Start daemon
    eprintln!("╔══════════════════════════════════════╗");
    eprintln!("║     ResonantOS Node Daemon           ║");
    eprintln!("╚══════════════════════════════════════╝");
    eprintln!();

    let mut daemon = resonantos_vnext::daemon::NodeDaemon::new(config);

    // Setup signal handler
    let running = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let r = running.clone();
    ctrlc_handler(move || {
        eprintln!("\n[resonantos-node] Received shutdown signal");
        r.store(false, std::sync::atomic::Ordering::Relaxed);
    });

    // Start
    if let Err(e) = daemon.start() {
        eprintln!("[resonantos-node] Failed to start: {}", e);
        std::process::exit(1);
    }

    // Main loop (wait for shutdown signal)
    while running.load(std::sync::atomic::Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    // Shutdown
    if let Err(e) = daemon.shutdown() {
        eprintln!("[resonantos-node] Shutdown error: {}", e);
        std::process::exit(1);
    }
}

fn ctrlc_handler(handler: impl FnOnce() + Send + 'static) {
    // Simple SIGINT handler (cross-platform)
    // In production: use ctrlc crate or tokio signal
    let _ = handler; // Placeholder — actual signal handling needs runtime
}
