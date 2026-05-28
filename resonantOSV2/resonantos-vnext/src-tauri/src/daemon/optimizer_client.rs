// Optimizer Client — receives and dispatches commands from the mesh optimizer.

/// Commands the optimizer can send to this node.
#[derive(Debug, Clone, PartialEq)]
pub enum OptimizerCommand {
    LoadModel { model_id: String, source_url: Option<String> },
    UnloadModel { model_id: String },
    RunInference { request_id: String, model_id: String, prompt: String },
    GetStatus,
    Shutdown,
    Unknown { raw: String },
}

/// Result of executing a command.
#[derive(Debug, Clone)]
pub enum CommandResult {
    Success { message: String },
    Error { reason: String },
    StatusReport { json: String },
}

/// Optimizer client that listens for and dispatches commands.
pub struct OptimizerClient {
    commands_received: u64,
    commands_failed: u64,
}

impl OptimizerClient {
    pub fn new() -> Self {
        Self { commands_received: 0, commands_failed: 0 }
    }

    /// Parse a raw message into an OptimizerCommand.
    pub fn parse_command(raw: &str) -> OptimizerCommand {
        // Simple command parsing (in production: MessagePack or JSON)
        let parts: Vec<&str> = raw.splitn(2, ':').collect();
        match parts.first().map(|s| s.trim()) {
            Some("load") => OptimizerCommand::LoadModel {
                model_id: parts.get(1).unwrap_or(&"").trim().to_string(),
                source_url: None,
            },
            Some("unload") => OptimizerCommand::UnloadModel {
                model_id: parts.get(1).unwrap_or(&"").trim().to_string(),
            },
            Some("status") => OptimizerCommand::GetStatus,
            Some("shutdown") => OptimizerCommand::Shutdown,
            Some("infer") => {
                let payload = parts.get(1).unwrap_or(&"").trim();
                let infer_parts: Vec<&str> = payload.splitn(2, '|').collect();
                OptimizerCommand::RunInference {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    model_id: infer_parts.first().unwrap_or(&"").trim().to_string(),
                    prompt: infer_parts.get(1).unwrap_or(&"").trim().to_string(),
                }
            }
            _ => OptimizerCommand::Unknown { raw: raw.to_string() },
        }
    }

    /// Record a received command.
    pub fn record_received(&mut self) {
        self.commands_received += 1;
    }

    /// Record a failed command.
    pub fn record_failed(&mut self) {
        self.commands_failed += 1;
    }

    /// Get stats.
    pub fn stats(&self) -> (u64, u64) {
        (self.commands_received, self.commands_failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_load_command() {
        let cmd = OptimizerClient::parse_command("load: llama-7b");
        assert!(matches!(cmd, OptimizerCommand::LoadModel { model_id, .. } if model_id == "llama-7b"));
    }

    #[test]
    fn test_parse_unload_command() {
        let cmd = OptimizerClient::parse_command("unload: qwen-14b");
        assert!(matches!(cmd, OptimizerCommand::UnloadModel { model_id } if model_id == "qwen-14b"));
    }

    #[test]
    fn test_parse_status_command() {
        let cmd = OptimizerClient::parse_command("status");
        assert_eq!(cmd, OptimizerCommand::GetStatus);
    }

    #[test]
    fn test_parse_shutdown_command() {
        let cmd = OptimizerClient::parse_command("shutdown");
        assert_eq!(cmd, OptimizerCommand::Shutdown);
    }

    #[test]
    fn test_parse_infer_command() {
        let cmd = OptimizerClient::parse_command("infer: llama-7b|Hello world");
        if let OptimizerCommand::RunInference { model_id, prompt, .. } = cmd {
            assert_eq!(model_id, "llama-7b");
            assert_eq!(prompt, "Hello world");
        } else {
            panic!("Expected RunInference");
        }
    }

    #[test]
    fn test_parse_unknown_command() {
        let cmd = OptimizerClient::parse_command("garbage xyz");
        assert!(matches!(cmd, OptimizerCommand::Unknown { .. }));
    }

    #[test]
    fn test_stats_tracking() {
        let mut client = OptimizerClient::new();
        client.record_received();
        client.record_received();
        client.record_failed();
        assert_eq!(client.stats(), (2, 1));
    }
}
