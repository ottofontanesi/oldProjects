// IPC Companion Commands — phone status, assignments, pairing
//
// 4 commands for managing phone companion nodes.

use super::state::AppState;
use super::types::{
    CompanionAssignment, CompanionPhoneStatus, PairingTokenResponse, UnpairResponse,
};

/// Get all paired phone companions with their current status.
pub async fn get_companion_status(
    state: &AppState,
) -> Result<Vec<CompanionPhoneStatus>, String> {
    let service_guard = state.companion_service.read().await;
    let _service = service_guard
        .as_ref()
        .ok_or_else(|| "Companion service not initialized. Please wait for startup to complete.".to_string())?;

    // In a full implementation, this would query the companion service for all paired phones.
    // For now, return an empty list (no phones paired yet).
    Ok(Vec::new())
}

/// Get layer assignments for a specific phone.
pub async fn get_companion_assignments(
    state: &AppState,
    node_id: String,
) -> Result<Vec<CompanionAssignment>, String> {
    let service_guard = state.companion_service.read().await;
    let service = service_guard
        .as_ref()
        .ok_or_else(|| "Companion service not initialized. Please wait for startup to complete.".to_string())?;

    let _node_uuid: uuid::Uuid = node_id
        .parse()
        .map_err(|_| format!("Invalid node_id: '{}'", node_id))?;

    // Query the assignment manager for this phone's assignments
    let loaded_models = service.assignment_manager().loaded_models();
    let result: Vec<CompanionAssignment> = loaded_models
        .iter()
        .map(|model_id| CompanionAssignment {
            model_id: model_id.clone(),
            layer_range: (0, 0), // Full model assignments don't have layer ranges
            memory_usage_mb: 0,  // Size not tracked per-model in assignment manager
            session_id: uuid::Uuid::new_v4().to_string(),
            protocol: "full".to_string(),
        })
        .collect();

    Ok(result)
}

/// Unpair a phone companion.
pub async fn unpair_companion(
    state: &AppState,
    node_id: String,
) -> Result<UnpairResponse, String> {
    let mut service_guard = state.companion_service.write().await;
    let service = service_guard
        .as_mut()
        .ok_or_else(|| "Companion service not initialized. Please wait for startup to complete.".to_string())?;

    let node_uuid: uuid::Uuid = node_id
        .parse()
        .map_err(|_| format!("Invalid node_id: '{}'", node_id))?;

    // Check if this is the service's own node
    if service.node_id() == node_uuid {
        service.stop();
        Ok(UnpairResponse {
            success: true,
            node_id,
            device_name: "local-companion".to_string(),
        })
    } else {
        Err(format!("Companion node '{}' not found", node_id))
    }
}

/// Generate a new pairing token for QR code display.
pub async fn get_pairing_token(
    state: &AppState,
) -> Result<PairingTokenResponse, String> {
    let _service_guard = state.companion_service.read().await;
    // Pairing token generation doesn't require the service to be initialized
    // since it's used to initiate the pairing process.

    let token = uuid::Uuid::new_v4().to_string();
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let expires_at_ms = now_ms + 5 * 60 * 1000; // 5 minutes

    let qr_data = format!("resonant://pair?token={}&expires={}", token, expires_at_ms);

    Ok(PairingTokenResponse {
        token,
        qr_data,
        expires_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::service::CompanionService;

    async fn make_state_with_companion() -> AppState {
        let state = AppState::new();
        let node_id = uuid::Uuid::new_v4();
        let mut service = CompanionService::new(node_id);
        service.initialize().unwrap();
        *state.companion_service.write().await = Some(service);
        state
    }

    #[tokio::test]
    async fn test_get_companion_status_with_service() {
        let state = make_state_with_companion().await;
        let result = get_companion_status(&state).await;
        assert!(result.is_ok());
        // No phones paired yet, so empty list
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_companion_status_uninitialized() {
        let state = AppState::new();
        let result = get_companion_status(&state).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not initialized"));
    }

    #[tokio::test]
    async fn test_unpair_unknown_node_returns_error() {
        let state = make_state_with_companion().await;
        let fake_id = uuid::Uuid::new_v4().to_string();
        let result = unpair_companion(&state, fake_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[tokio::test]
    async fn test_unpair_own_node_succeeds() {
        let state = AppState::new();
        let node_id = uuid::Uuid::new_v4();
        let mut service = CompanionService::new(node_id);
        service.initialize().unwrap();
        *state.companion_service.write().await = Some(service);

        let result = unpair_companion(&state, node_id.to_string()).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(resp.success);
        assert_eq!(resp.node_id, node_id.to_string());
    }

    #[tokio::test]
    async fn test_get_pairing_token_returns_valid_format() {
        let state = AppState::new();
        let result = get_pairing_token(&state).await;
        assert!(result.is_ok());
        let token_resp = result.unwrap();
        assert!(!token_resp.token.is_empty());
        assert!(token_resp.qr_data.starts_with("resonant://pair?token="));
        assert!(token_resp.expires_at_ms > 0);
    }

    #[tokio::test]
    async fn test_get_companion_assignments_with_service() {
        let state = make_state_with_companion().await;
        let node_id = uuid::Uuid::new_v4().to_string();
        let result = get_companion_assignments(&state, node_id).await;
        assert!(result.is_ok());
        // No assignments yet
        assert!(result.unwrap().is_empty());
    }
}
