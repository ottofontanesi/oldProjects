// Control API — minimal localhost HTTP for daemon management.
// Binds to 127.0.0.1:9742 only (not network-exposed).

use super::DaemonStatus;

/// API request types.
#[derive(Debug, Clone, PartialEq)]
pub enum ApiRequest {
    GetStatus,
    GetModels,
    LoadModel { model_id: String },
    UnloadModel { model_id: String },
    Shutdown,
    GetConfig,
    UpdateConfig { json: String },
}

/// API response.
#[derive(Debug, Clone)]
pub struct ApiResponse {
    pub status_code: u16,
    pub body: String,
}

impl ApiResponse {
    pub fn ok(body: &str) -> Self {
        Self { status_code: 200, body: body.to_string() }
    }

    pub fn error(code: u16, message: &str) -> Self {
        Self { status_code: code, body: format!("{{\"error\":\"{}\"}}", message) }
    }
}

/// Control API server (minimal, localhost-only).
pub struct ControlApi {
    port: u16,
    bind_address: String,
    request_count: u64,
}

impl ControlApi {
    pub fn new(port: u16) -> Self {
        Self {
            port,
            bind_address: format!("127.0.0.1:{}", port),
            request_count: 0,
        }
    }

    /// Get the bind address.
    pub fn bind_address(&self) -> &str {
        &self.bind_address
    }

    /// Check if an address is localhost (security check).
    pub fn is_localhost(addr: &str) -> bool {
        addr.starts_with("127.0.0.1")
            || addr.starts_with("localhost")
            || addr.starts_with("::1")
    }

    /// Parse an HTTP request path into an ApiRequest.
    pub fn parse_request(method: &str, path: &str, body: Option<&str>) -> Option<ApiRequest> {
        match (method, path) {
            ("GET", "/status") => Some(ApiRequest::GetStatus),
            ("GET", "/models") => Some(ApiRequest::GetModels),
            ("POST", "/load") => {
                let model_id = body.unwrap_or("").trim().to_string();
                Some(ApiRequest::LoadModel { model_id })
            }
            ("POST", "/unload") => {
                let model_id = body.unwrap_or("").trim().to_string();
                Some(ApiRequest::UnloadModel { model_id })
            }
            ("POST", "/shutdown") => Some(ApiRequest::Shutdown),
            ("GET", "/config") => Some(ApiRequest::GetConfig),
            ("POST", "/config") => {
                let json = body.unwrap_or("{}").to_string();
                Some(ApiRequest::UpdateConfig { json })
            }
            _ => None,
        }
    }

    /// Format a DaemonStatus as JSON.
    pub fn format_status(status: &DaemonStatus) -> String {
        format!(
            "{{\"node_id\":\"{}\",\"running\":{},\"uptime_secs\":{},\"models_loaded\":{},\"low_power\":{},\"listen_port\":{}}}",
            status.node_id,
            status.running,
            status.uptime_secs,
            format!("[{}]", status.models_loaded.iter().map(|m| format!("\"{}\"", m)).collect::<Vec<_>>().join(",")),
            status.low_power,
            status.listen_port,
        )
    }

    /// Record a request.
    pub fn record_request(&mut self) {
        self.request_count += 1;
    }

    /// Get request count.
    pub fn request_count(&self) -> u64 {
        self.request_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_get_status() {
        let req = ControlApi::parse_request("GET", "/status", None);
        assert_eq!(req, Some(ApiRequest::GetStatus));
    }

    #[test]
    fn test_parse_post_load() {
        let req = ControlApi::parse_request("POST", "/load", Some("llama-7b"));
        assert_eq!(req, Some(ApiRequest::LoadModel { model_id: "llama-7b".to_string() }));
    }

    #[test]
    fn test_parse_unknown_path() {
        let req = ControlApi::parse_request("GET", "/unknown", None);
        assert_eq!(req, None);
    }

    #[test]
    fn test_is_localhost() {
        assert!(ControlApi::is_localhost("127.0.0.1:9742"));
        assert!(ControlApi::is_localhost("localhost:9742"));
        assert!(!ControlApi::is_localhost("192.168.1.10:9742"));
        assert!(!ControlApi::is_localhost("0.0.0.0:9742"));
    }

    #[test]
    fn test_format_status() {
        let status = DaemonStatus {
            node_id: uuid::Uuid::nil(),
            running: true,
            uptime_secs: 120,
            models_loaded: vec!["llama-7b".to_string()],
            low_power: false,
            listen_port: 9741,
        };
        let json = ControlApi::format_status(&status);
        assert!(json.contains("\"running\":true"));
        assert!(json.contains("\"uptime_secs\":120"));
        assert!(json.contains("llama-7b"));
    }

    #[test]
    fn test_api_response_ok() {
        let resp = ApiResponse::ok("{\"status\":\"ok\"}");
        assert_eq!(resp.status_code, 200);
    }

    #[test]
    fn test_api_response_error() {
        let resp = ApiResponse::error(404, "Not found");
        assert_eq!(resp.status_code, 404);
        assert!(resp.body.contains("Not found"));
    }

    #[test]
    fn test_bind_address() {
        let api = ControlApi::new(9742);
        assert_eq!(api.bind_address(), "127.0.0.1:9742");
    }
}
