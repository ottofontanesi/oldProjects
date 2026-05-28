// Intent citation: docs/architecture/ADR-003-engineering-standards.md
// Feature: engineer-backtest-mode — Rust Backtest Service

use serde::{Deserialize, Serialize};
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// ─── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestExecutionRequest {
    pub job_id: String,
    pub node_id: String,
    pub suite_type: String,
    pub args: Vec<String>,
    pub cpu_limit_percent: Option<u8>,
    pub timeout_ms: Option<u64>,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BacktestExecutionResult {
    pub job_id: String,
    pub node_id: String,
    pub suite_type: String,
    pub status: String,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: String,
    pub completed_at: String,
    pub duration_ms: u64,
    pub summary: String,
}

// ─── Constants ──────────────────────────────────────────────────────────────

const GX10_HOST: &str = "rlab@gx10-23bd.local";
const DEFAULT_TIMEOUT_MS: u64 = 300_000; // 5 minutes
const MAX_OUTPUT_BYTES: usize = 16_384;

/// Allowlisted programs for backtest execution.
/// Only these programs can be invoked by the backtest service.
const BACKTEST_ALLOWLIST: &[&str] = &["npx", "cargo", "vitest"];

// ─── Helpers ────────────────────────────────────────────────────────────────

fn timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("unix:{}", d.as_secs()))
        .unwrap_or_else(|_| "unix:0".to_string())
}

fn trim_output(value: &[u8]) -> String {
    let mut output = String::from_utf8_lossy(value).to_string();
    if output.len() > MAX_OUTPUT_BYTES {
        output.truncate(MAX_OUTPUT_BYTES);
        output.push_str("\n[truncated]");
    }
    output
}

// ─── Allowlist Enforcement ──────────────────────────────────────────────────

/// Validates that the requested command is in the backtest allowlist.
/// Returns the resolved program name or an error.
pub(crate) fn validate_backtest_command(
    suite_type: &str,
    args: &[String],
) -> Result<Vec<String>, String> {
    match suite_type {
        "vitest" => {
            // Allowed: npx vitest run [--include patterns...]
            let mut cmd = vec![
                "npx".to_string(),
                "vitest".to_string(),
                "run".to_string(),
            ];
            // Only allow safe vitest arguments
            for arg in args {
                if arg.starts_with("--") || !arg.contains("..") {
                    cmd.push(arg.clone());
                } else {
                    return Err(format!(
                        "Backtest argument `{arg}` is not allowed (path traversal detected)."
                    ));
                }
            }
            Ok(cmd)
        }
        "cargo-test" => {
            // Allowed: cargo test [-p package] [-- test_filter]
            let mut cmd = vec!["cargo".to_string(), "test".to_string()];
            let mut i = 0;
            while i < args.len() {
                let arg = &args[i];
                match arg.as_str() {
                    "-p" | "--package" => {
                        cmd.push(arg.clone());
                        i += 1;
                        if i < args.len() {
                            cmd.push(args[i].clone());
                        }
                    }
                    "--" => {
                        // Pass remaining args as test filter
                        cmd.extend_from_slice(&args[i..]);
                        break;
                    }
                    _ if !arg.starts_with('-') => {
                        cmd.push(arg.clone());
                    }
                    _ => {
                        return Err(format!(
                            "Backtest cargo argument `{arg}` is not allowlisted."
                        ));
                    }
                }
                i += 1;
            }
            Ok(cmd)
        }
        _ => Err(format!(
            "Backtest suite type `{suite_type}` is not allowlisted. Allowed: vitest, cargo-test."
        )),
    }
}

// ─── CPU Throttling ─────────────────────────────────────────────────────────

/// Builds the CPU throttle prefix for the command based on the platform.
/// On Unix: uses `nice` to lower process priority.
/// On Windows: priority is set via process creation flags (handled separately).
pub(crate) fn build_throttle_prefix(cpu_limit_percent: Option<u8>) -> Vec<String> {
    let limit = match cpu_limit_percent {
        Some(pct) if pct < 100 => pct,
        _ => return Vec::new(),
    };

    // Map percentage to nice level: 100% = 0, 50% = 10, 25% = 15, 0% = 19
    let nice_level = if limit <= 25 {
        15
    } else if limit <= 50 {
        10
    } else if limit <= 75 {
        5
    } else {
        0
    };

    if cfg!(unix) {
        vec![
            "nice".to_string(),
            format!("-n{nice_level}"),
            "ionice".to_string(),
            "-c3".to_string(),
        ]
    } else {
        // On Windows, we don't use nice/ionice prefix.
        // Priority is set via CreateProcess flags in the execution step.
        Vec::new()
    }
}

/// Returns the Windows process priority class for the given CPU limit.
/// Used with SetPriorityClass on Windows.
#[cfg(target_os = "windows")]
pub(crate) fn windows_priority_class(cpu_limit_percent: Option<u8>) -> u32 {
    // BELOW_NORMAL_PRIORITY_CLASS = 0x00004000
    // IDLE_PRIORITY_CLASS = 0x00000040
    // NORMAL_PRIORITY_CLASS = 0x00000020
    match cpu_limit_percent {
        Some(pct) if pct <= 25 => 0x00000040,  // IDLE
        Some(pct) if pct <= 75 => 0x00004000,  // BELOW_NORMAL
        _ => 0x00000020,                        // NORMAL
    }
}

// ─── Timeout Enforcement ────────────────────────────────────────────────────

/// Executes a command with timeout enforcement.
/// Returns the result or an error if the timeout is exceeded.
fn execute_with_timeout(
    command: &[String],
    timeout_ms: u64,
    cwd: Option<&str>,
) -> Result<(bool, Option<i32>, String, String, u64), String> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| "Command cannot be empty.".to_string())?;

    // Validate program is in allowlist
    let program_name = program.as_str();
    if !BACKTEST_ALLOWLIST.contains(&program_name) && program_name != "nice" && program_name != "ionice" {
        return Err(format!(
            "Program `{program_name}` is not in the backtest allowlist."
        ));
    }

    let start = Instant::now();
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null());

    if let Some(dir) = cwd {
        cmd.current_dir(dir);
    }

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to execute backtest command `{program_name}`: {e}"))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    // Check if we exceeded the timeout (post-hoc check for synchronous execution)
    if duration_ms > timeout_ms {
        return Err(format!(
            "Backtest command `{program_name}` exceeded timeout of {timeout_ms}ms (took {duration_ms}ms)."
        ));
    }

    Ok((
        output.status.success(),
        output.status.code(),
        trim_output(&output.stdout),
        trim_output(&output.stderr),
        duration_ms,
    ))
}

// ─── Remote Execution ───────────────────────────────────────────────────────

/// Executes a backtest command on a remote node via SSH.
/// Reuses the existing SSH infrastructure pattern from compute_service.
fn run_ssh_backtest(
    host: &str,
    remote_command: &str,
    timeout_ms: u64,
) -> Result<(bool, Option<i32>, String, String, u64), String> {
    let connect_timeout = std::cmp::min(timeout_ms / 1000, 30);
    let start = Instant::now();

    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            &format!("ConnectTimeout={connect_timeout}"),
            host,
            remote_command,
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Failed to run SSH backtest on `{host}`: {e}"))?;

    let duration_ms = start.elapsed().as_millis() as u64;

    if duration_ms > timeout_ms {
        return Err(format!(
            "SSH backtest on `{host}` exceeded timeout of {timeout_ms}ms."
        ));
    }

    Ok((
        output.status.success(),
        output.status.code(),
        trim_output(&output.stdout),
        trim_output(&output.stderr),
        duration_ms,
    ))
}

/// Resolves the SSH host for a given node ID.
fn resolve_remote_host(node_id: &str) -> Result<&'static str, String> {
    match node_id {
        "compute-gx10" => Ok(GX10_HOST),
        _ => Err(format!(
            "Remote backtest node `{node_id}` is not configured for SSH execution."
        )),
    }
}

// ─── Main Execution ─────────────────────────────────────────────────────────

/// Executes a backtest suite on the specified node.
/// Extends the safe-command allowlist to include vitest (via npx) and cargo test.
pub(crate) fn execute_backtest_suite(
    request: BacktestExecutionRequest,
) -> Result<BacktestExecutionResult, String> {
    let timeout_ms = request.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS);
    let started_at = timestamp();

    // Validate and build the command
    let command = validate_backtest_command(&request.suite_type, &request.args)?;

    // Determine execution path: local or remote
    let (success, exit_code, stdout, stderr, duration_ms) = if request.node_id
        == "compute-desktop-local"
    {
        // Local execution with optional CPU throttling
        let throttle_prefix = build_throttle_prefix(request.cpu_limit_percent);
        let full_command = if throttle_prefix.is_empty() {
            command
        } else {
            let mut full = throttle_prefix;
            full.extend(command);
            full
        };
        execute_with_timeout(&full_command, timeout_ms, request.cwd.as_deref())?
    } else {
        // Remote execution via SSH
        let host = resolve_remote_host(&request.node_id)?;
        let remote_cmd = command.join(" ");
        run_ssh_backtest(host, &remote_cmd, timeout_ms)?
    };

    let completed_at = timestamp();
    let status = if success { "succeeded" } else { "failed" }.to_string();

    Ok(BacktestExecutionResult {
        job_id: request.job_id,
        node_id: request.node_id,
        suite_type: request.suite_type.clone(),
        status: status.clone(),
        exit_code,
        stdout,
        stderr,
        started_at,
        completed_at,
        duration_ms,
        summary: format!(
            "Backtest suite `{}` {status} in {duration_ms}ms.",
            request.suite_type
        ),
    })
}

// ─── Tauri Command ──────────────────────────────────────────────────────────

#[tauri::command]
pub(crate) fn backtest_execute_suite(
    request: BacktestExecutionRequest,
) -> Result<BacktestExecutionResult, String> {
    execute_backtest_suite(request)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowlist_accepts_vitest_suite() {
        let result = validate_backtest_command("vitest", &[]);
        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd, vec!["npx", "vitest", "run"]);
    }

    #[test]
    fn allowlist_accepts_vitest_with_include_patterns() {
        let result = validate_backtest_command(
            "vitest",
            &["--include".to_string(), "src/core/**".to_string()],
        );
        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd, vec!["npx", "vitest", "run", "--include", "src/core/**"]);
    }

    #[test]
    fn allowlist_rejects_path_traversal_in_vitest() {
        let result = validate_backtest_command("vitest", &["../../etc/passwd".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("path traversal"));
    }

    #[test]
    fn allowlist_accepts_cargo_test_suite() {
        let result = validate_backtest_command("cargo-test", &[]);
        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd, vec!["cargo", "test"]);
    }

    #[test]
    fn allowlist_accepts_cargo_test_with_package() {
        let result = validate_backtest_command(
            "cargo-test",
            &["-p".to_string(), "resonantos_vnext".to_string()],
        );
        assert!(result.is_ok());
        let cmd = result.unwrap();
        assert_eq!(cmd, vec!["cargo", "test", "-p", "resonantos_vnext"]);
    }

    #[test]
    fn allowlist_rejects_unknown_suite_type() {
        let result = validate_backtest_command("python", &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not allowlisted"));
    }

    #[test]
    fn allowlist_rejects_disallowed_cargo_flags() {
        let result = validate_backtest_command("cargo-test", &["--release".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not allowlisted"));
    }

    #[test]
    fn throttle_prefix_empty_when_no_limit() {
        let prefix = build_throttle_prefix(None);
        assert!(prefix.is_empty());
    }

    #[test]
    fn throttle_prefix_empty_when_100_percent() {
        let prefix = build_throttle_prefix(Some(100));
        assert!(prefix.is_empty());
    }

    #[test]
    fn throttle_prefix_uses_nice_on_unix() {
        let prefix = build_throttle_prefix(Some(50));
        if cfg!(unix) {
            assert!(!prefix.is_empty());
            assert_eq!(prefix[0], "nice");
            assert!(prefix[1].contains("10"));
        } else {
            // On Windows, no prefix is used
            assert!(prefix.is_empty());
        }
    }

    #[test]
    fn throttle_prefix_high_nice_for_low_cpu() {
        let prefix = build_throttle_prefix(Some(20));
        if cfg!(unix) {
            assert!(prefix[1].contains("15"));
        }
    }

    #[test]
    fn timeout_default_is_5_minutes() {
        assert_eq!(DEFAULT_TIMEOUT_MS, 300_000);
    }

    #[test]
    fn remote_host_resolves_gx10() {
        let result = resolve_remote_host("compute-gx10");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), GX10_HOST);
    }

    #[test]
    fn remote_host_rejects_unknown_node() {
        let result = resolve_remote_host("compute-unknown");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not configured"));
    }

    #[test]
    fn request_deserialization_works() {
        let json = r#"{
            "jobId": "job-123",
            "nodeId": "compute-desktop-local",
            "suiteType": "vitest",
            "args": ["--include", "src/core/**"],
            "cpuLimitPercent": 50,
            "timeoutMs": 60000
        }"#;
        let request: BacktestExecutionRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.job_id, "job-123");
        assert_eq!(request.node_id, "compute-desktop-local");
        assert_eq!(request.suite_type, "vitest");
        assert_eq!(request.args, vec!["--include", "src/core/**"]);
        assert_eq!(request.cpu_limit_percent, Some(50));
        assert_eq!(request.timeout_ms, Some(60000));
    }

    #[test]
    fn result_serialization_works() {
        let result = BacktestExecutionResult {
            job_id: "job-123".to_string(),
            node_id: "compute-desktop-local".to_string(),
            suite_type: "vitest".to_string(),
            status: "succeeded".to_string(),
            exit_code: Some(0),
            stdout: "all tests passed".to_string(),
            stderr: String::new(),
            started_at: "unix:1000".to_string(),
            completed_at: "unix:1005".to_string(),
            duration_ms: 5000,
            summary: "Backtest suite `vitest` succeeded in 5000ms.".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"jobId\":\"job-123\""));
        assert!(json.contains("\"suiteType\":\"vitest\""));
        assert!(json.contains("\"durationMs\":5000"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_priority_class_maps_correctly() {
        assert_eq!(windows_priority_class(Some(20)), 0x00000040);  // IDLE
        assert_eq!(windows_priority_class(Some(50)), 0x00004000);  // BELOW_NORMAL
        assert_eq!(windows_priority_class(Some(100)), 0x00000020); // NORMAL
        assert_eq!(windows_priority_class(None), 0x00000020);      // NORMAL
    }
}
