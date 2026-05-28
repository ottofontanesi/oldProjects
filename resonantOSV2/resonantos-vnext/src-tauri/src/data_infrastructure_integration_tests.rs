//! Integration tests for Data Infrastructure services.
//!
//! Contains:
//! - 7.5: Health monitor probe cycle with mocked HTTP responses
//! - 7.6: Cost ledger flow: record → query → verify aggregation
//! - 7.7: Federated memory: write → query → eviction at capacity
//! - 7.8: Health degradation → shell notification emission
//! - 7.9: Graceful degradation: services return errors without crashing when databases are unavailable

#[cfg(test)]
mod integration_tests {
    use crate::cost_ledger_service::{
        initialize_cost_ledger_db, record_cost_entry, query_cost_dashboard,
        cost_ledger_projection_from_db, estimate_cost_usd, CostRecord, CostLedgerQuery,
    };
    use crate::federated_memory_service::{
        initialize_federated_memory_db, federated_memory_write, federated_memory_query_db,
        federated_memory_read_by_id_db, get_fact_count, FactWriteRequest, FactQuery,
        MAX_STORE_SIZE,
    };
    use crate::health_monitor::{
        compute_rolling_average, determine_health_state, detect_latency_spike,
        select_fallback_route, build_degradation_event, HealthMonitorConfig,
    };
    use rusqlite::Connection;

    // ─── Helpers ────────────────────────────────────────────────────────────

    fn create_cost_ledger_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_cost_ledger_db(&conn).unwrap();
        conn
    }

    fn create_federated_memory_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_federated_memory_db(&conn).unwrap();
        conn
    }

    fn make_cost_record(id: &str, agent_id: &str, task_type: &str, day: &str) -> CostRecord {
        let prompt_tokens = 1000;
        let completion_tokens = 500;
        let cost = estimate_cost_usd("paid-api", prompt_tokens, completion_tokens);
        CostRecord {
            id: id.to_string(),
            recorded_at: format!("{}T10:00:00Z", day),
            agent_id: agent_id.to_string(),
            task_type: task_type.to_string(),
            provider_id: "openai-main".to_string(),
            model: "gpt-4o".to_string(),
            cost_posture: "paid-api".to_string(),
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            estimated_cost_usd: cost,
            duration_ms: Some(1200),
        }
    }

    // ─── 7.5: Health Monitor Probe Cycle ────────────────────────────────────

    /// Integration test: health monitor probe cycle with mocked HTTP responses.
    /// Tests the full probe → state transition → degradation detection flow
    /// without actual network calls by exercising the pure logic functions.
    #[test]
    fn test_health_monitor_probe_cycle_ready_state() {
        let config = HealthMonitorConfig::default();

        // Simulate a sequence of successful probes with normal latencies
        let latencies: Vec<u64> = vec![100, 110, 105, 95, 108, 102, 115, 98, 107, 103];
        let rolling_avg = compute_rolling_average(&latencies, config.rolling_window_size);

        // Verify rolling average is computed correctly
        let expected_avg: f64 = latencies.iter().sum::<u64>() as f64 / latencies.len() as f64;
        assert!(
            (rolling_avg - expected_avg).abs() < 0.01,
            "Rolling average should be {}, got {}",
            expected_avg,
            rolling_avg
        );

        // New probe with normal latency → should remain "ready"
        let new_latency = 112;
        let (state, failures) = determine_health_state(
            true,
            new_latency,
            rolling_avg,
            0, // no prior failures
            &config,
        );
        assert_eq!(state, "ready");
        assert_eq!(failures, 0);

        // Verify no latency spike detected
        assert!(!detect_latency_spike(new_latency, rolling_avg, config.latency_spike_multiplier));
    }

    #[test]
    fn test_health_monitor_probe_cycle_degradation_on_failure() {
        let config = HealthMonitorConfig::default();

        // Simulate a failed probe (HTTP error or timeout)
        let (state, failures) = determine_health_state(
            false, // probe failed
            0,
            100.0, // existing rolling average
            0,     // first failure
            &config,
        );
        assert_eq!(state, "degraded");
        assert_eq!(failures, 1);

        // Second consecutive failure
        let (state, failures) = determine_health_state(false, 0, 100.0, 1, &config);
        assert_eq!(state, "degraded");
        assert_eq!(failures, 2);

        // Third consecutive failure → unavailable
        let (state, failures) = determine_health_state(false, 0, 100.0, 2, &config);
        assert_eq!(state, "unavailable");
        assert_eq!(failures, 3);
    }

    #[test]
    fn test_health_monitor_probe_cycle_latency_spike() {
        let config = HealthMonitorConfig::default();

        let rolling_avg = 100.0;
        let spike_latency = 250; // > 2× rolling average

        // Detect spike
        assert!(detect_latency_spike(spike_latency, rolling_avg, config.latency_spike_multiplier));

        // State should be degraded
        let (state, failures) = determine_health_state(
            true,
            spike_latency,
            rolling_avg,
            0,
            &config,
        );
        assert_eq!(state, "degraded");
        assert_eq!(failures, 0); // success resets failure count
    }

    #[test]
    fn test_health_monitor_probe_cycle_recovery() {
        let config = HealthMonitorConfig::default();

        // After failures, a successful probe with normal latency → ready
        let (state, failures) = determine_health_state(
            true,  // probe succeeded
            100,   // normal latency
            100.0, // rolling average
            2,     // had 2 consecutive failures before
            &config,
        );
        assert_eq!(state, "ready");
        assert_eq!(failures, 0); // reset on success
    }

    #[test]
    fn test_health_monitor_fallback_selection_in_probe_cycle() {
        let chain = vec![
            "provider-primary".to_string(),
            "provider-secondary".to_string(),
            "provider-tertiary".to_string(),
        ];

        // Primary degraded → select secondary
        let fallback = select_fallback_route(&chain, "provider-primary");
        assert_eq!(fallback, Some("provider-secondary".to_string()));

        // Secondary degraded → select tertiary
        let fallback = select_fallback_route(&chain, "provider-secondary");
        assert_eq!(fallback, Some("provider-tertiary".to_string()));

        // Tertiary degraded → no fallback
        let fallback = select_fallback_route(&chain, "provider-tertiary");
        assert_eq!(fallback, None);
    }

    // ─── 7.6: Provider Chat → Cost Event → Ledger Write → Dashboard Query ──

    #[test]
    fn test_cost_ledger_record_write_and_dashboard_query() {
        let conn = create_cost_ledger_db();

        // Simulate provider chat completion → cost record creation
        let record = make_cost_record("chat-rec-1", "strategist.core", "chat", "2026-06-15");
        record_cost_entry(&conn, &record).unwrap();

        // Query the dashboard
        let query = CostLedgerQuery {
            period_type: Some("day".to_string()),
            agent_id: None,
            task_type: None,
            from_date: None,
            to_date: None,
            limit: None,
        };
        let dashboard = query_cost_dashboard(&conn, &query).unwrap();

        // Verify the record appears in recent records
        assert_eq!(dashboard.recent_records.len(), 1);
        assert_eq!(dashboard.recent_records[0].id, "chat-rec-1");
        assert_eq!(dashboard.recent_records[0].agent_id, "strategist.core");
        assert_eq!(dashboard.recent_records[0].task_type, "chat");

        // Verify aggregation was created
        assert!(!dashboard.aggregations.is_empty());
        let day_agg = dashboard
            .aggregations
            .iter()
            .find(|a| a.period == "2026-06-15" && a.period_type == "day")
            .expect("Should have daily aggregation");
        assert_eq!(day_agg.agent_id, "strategist.core");
        assert_eq!(day_agg.total_tokens, 1500);
        assert_eq!(day_agg.record_count, 1);
    }

    #[test]
    fn test_cost_ledger_multiple_records_aggregation() {
        let conn = create_cost_ledger_db();

        // Write multiple records for the same agent/day
        for i in 0..5 {
            let record = make_cost_record(
                &format!("rec-{}", i),
                "strategist.core",
                "chat",
                "2026-06-15",
            );
            record_cost_entry(&conn, &record).unwrap();
        }

        // Write records for a different agent
        for i in 0..3 {
            let record = make_cost_record(
                &format!("other-rec-{}", i),
                "logician.core",
                "verification",
                "2026-06-15",
            );
            record_cost_entry(&conn, &record).unwrap();
        }

        // Query all daily aggregations
        let query = CostLedgerQuery {
            period_type: Some("day".to_string()),
            agent_id: None,
            task_type: None,
            from_date: None,
            to_date: None,
            limit: None,
        };
        let dashboard = query_cost_dashboard(&conn, &query).unwrap();

        // Verify aggregations
        let strategist_agg = dashboard
            .aggregations
            .iter()
            .find(|a| a.agent_id == "strategist.core" && a.period_type == "day")
            .expect("Should have strategist aggregation");
        assert_eq!(strategist_agg.record_count, 5);
        assert_eq!(strategist_agg.total_tokens, 5 * 1500);

        let logician_agg = dashboard
            .aggregations
            .iter()
            .find(|a| a.agent_id == "logician.core" && a.period_type == "day")
            .expect("Should have logician aggregation");
        assert_eq!(logician_agg.record_count, 3);
        assert_eq!(logician_agg.total_tokens, 3 * 1500);

        // Verify recent records count
        assert_eq!(dashboard.recent_records.len(), 8);
    }

    #[test]
    fn test_cost_ledger_projection_with_data() {
        let conn = create_cost_ledger_db();

        // Write records across multiple days (projection uses last 7 days)
        let record = make_cost_record("proj-rec-1", "strategist.core", "chat", "2026-06-15");
        record_cost_entry(&conn, &record).unwrap();

        // Query projection
        let projection = cost_ledger_projection_from_db(&conn).unwrap();

        // Projection should be computed (may be 0 if date is outside 7-day window
        // relative to 'now', but the function should not error)
        assert_eq!(projection.rolling_window_days, 7);
        assert!(projection.daily_average_usd >= 0.0);
        assert!(projection.projected_monthly_usd >= 0.0);
    }

    #[test]
    fn test_cost_ledger_filter_by_agent() {
        let conn = create_cost_ledger_db();

        record_cost_entry(&conn, &make_cost_record("r1", "agent-a", "chat", "2026-06-15")).unwrap();
        record_cost_entry(&conn, &make_cost_record("r2", "agent-b", "chat", "2026-06-15")).unwrap();
        record_cost_entry(&conn, &make_cost_record("r3", "agent-a", "review", "2026-06-15")).unwrap();

        let query = CostLedgerQuery {
            period_type: Some("day".to_string()),
            agent_id: Some("agent-a".to_string()),
            task_type: None,
            from_date: None,
            to_date: None,
            limit: None,
        };
        let dashboard = query_cost_dashboard(&conn, &query).unwrap();

        // Only agent-a records should appear
        for record in &dashboard.recent_records {
            assert_eq!(record.agent_id, "agent-a");
        }
        assert_eq!(dashboard.recent_records.len(), 2);
    }

    // ─── 7.7: Federated Memory Write → Query → Eviction at Capacity ────────

    #[test]
    fn test_federated_memory_write_and_query() {
        let conn = create_federated_memory_db();
        let agent_id = "strategist.core";

        // Write a fact
        let request = FactWriteRequest {
            agent_id: agent_id.to_string(),
            category: "system-config".to_string(),
            content: "Default model is gpt-4o".to_string(),
            confidence: 0.95,
            ttl_seconds: 86400,
        };
        let result = federated_memory_write(&conn, &request).unwrap();
        assert!(result.accepted);
        assert!(result.error.is_none());
        assert!(!result.id.is_empty());

        // Query the fact back
        let query = FactQuery {
            category: Some("system-config".to_string()),
            source_agent: None,
            min_confidence: None,
            max_age_seconds: None,
            limit: None,
        };
        let facts = federated_memory_query_db(&conn, agent_id, &query).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].content, "Default model is gpt-4o");
        assert_eq!(facts[0].category, "system-config");

        // Read by ID
        let fact = federated_memory_read_by_id_db(&conn, agent_id, &result.id)
            .unwrap()
            .expect("Should find fact by ID");
        assert_eq!(fact.id, result.id);
        assert_eq!(fact.content, "Default model is gpt-4o");
    }

    #[test]
    fn test_federated_memory_eviction_at_capacity() {
        let conn = create_federated_memory_db();
        let agent_id = "strategist.core";

        // Fill the store to capacity (50 records)
        for i in 0..MAX_STORE_SIZE {
            let request = FactWriteRequest {
                agent_id: agent_id.to_string(),
                category: "system-config".to_string(),
                content: format!("fact number {}", i),
                confidence: 0.8,
                ttl_seconds: 86400, // 24 hours, won't expire during test
            };
            let result = federated_memory_write(&conn, &request).unwrap();
            assert!(result.accepted, "Write {} should succeed", i);
        }

        // Verify at capacity
        let count = get_fact_count(&conn).unwrap();
        assert_eq!(count, MAX_STORE_SIZE);

        // Write the 51st record — should trigger eviction
        let request = FactWriteRequest {
            agent_id: agent_id.to_string(),
            category: "system-config".to_string(),
            content: "the 51st fact that triggers eviction".to_string(),
            confidence: 0.9,
            ttl_seconds: 86400,
        };
        let result = federated_memory_write(&conn, &request).unwrap();
        assert!(result.accepted, "51st write should succeed after eviction");
        assert!(
            !result.evicted_ids.is_empty(),
            "Should have evicted at least one record"
        );

        // Store should still be at or below capacity
        let count = get_fact_count(&conn).unwrap();
        assert!(
            count <= MAX_STORE_SIZE,
            "Store size {} should not exceed max {}",
            count,
            MAX_STORE_SIZE
        );

        // The new fact should be queryable
        let fact = federated_memory_read_by_id_db(&conn, agent_id, &result.id)
            .unwrap()
            .expect("New fact should be readable");
        assert_eq!(fact.content, "the 51st fact that triggers eviction");
    }

    #[test]
    fn test_federated_memory_query_filtering() {
        let conn = create_federated_memory_db();
        let agent_id = "strategist.core";

        // Write facts with different categories
        let categories = ["system-config", "provider-state", "user-preference", "architecture-decision"];
        for (i, cat) in categories.iter().enumerate() {
            let request = FactWriteRequest {
                agent_id: agent_id.to_string(),
                category: cat.to_string(),
                content: format!("fact for category {}", cat),
                confidence: (i as f64 + 1.0) * 0.2,
                ttl_seconds: 3600,
            };
            federated_memory_write(&conn, &request).unwrap();
            // Small pause to ensure distinct timestamps
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        // Query with category filter
        let query = FactQuery {
            category: Some("system-config".to_string()),
            source_agent: None,
            min_confidence: None,
            max_age_seconds: None,
            limit: None,
        };
        let results = federated_memory_query_db(&conn, agent_id, &query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].category, "system-config");

        // Query with min_confidence filter
        let query = FactQuery {
            category: None,
            source_agent: None,
            min_confidence: Some(0.5),
            max_age_seconds: None,
            limit: None,
        };
        let results = federated_memory_query_db(&conn, agent_id, &query).unwrap();
        for fact in &results {
            assert!(fact.confidence >= 0.5);
        }
    }

    // ─── 7.8: Health Degradation → Shell Notification Emission ──────────────

    #[test]
    fn test_health_degradation_triggers_notification_event() {
        let config = HealthMonitorConfig::default();

        // Simulate consecutive failures leading to degradation
        let mut consecutive_failures = 0u32;
        let mut degradation_events: Vec<crate::health_monitor::DegradationEvent> = Vec::new();

        // Simulate 3 consecutive probe failures
        for _ in 0..3 {
            let (state, new_failures) = determine_health_state(
                false, // probe failed
                0,
                100.0,
                consecutive_failures,
                &config,
            );
            consecutive_failures = new_failures;

            // On degradation or unavailable, build a notification event
            if state == "degraded" || state == "unavailable" {
                let fallback = select_fallback_route(
                    &["provider-primary".to_string(), "provider-secondary".to_string()],
                    "provider-primary",
                );
                let event = build_degradation_event(
                    "provider-primary",
                    "node-cloud-1",
                    if state == "unavailable" { "unavailable" } else { "error-response" },
                    fallback,
                    "initiated",
                );
                degradation_events.push(event);
            }
        }

        // Should have emitted 3 degradation events (one per failure)
        assert_eq!(degradation_events.len(), 3);

        // First two should be "error-response" (degraded state)
        assert_eq!(degradation_events[0].severity, "error-response");
        assert_eq!(degradation_events[1].severity, "error-response");

        // Third should be "unavailable" (3 consecutive failures)
        assert_eq!(degradation_events[2].severity, "unavailable");

        // All events should have required fields
        for event in &degradation_events {
            assert!(!event.provider_profile_id.is_empty());
            assert!(!event.runtime_node_id.is_empty());
            assert!(!event.detected_at.is_empty());
            assert_eq!(event.pre_warm_status, "initiated");
            assert_eq!(event.fallback_route_id, Some("provider-secondary".to_string()));
        }
    }

    #[test]
    fn test_health_degradation_latency_spike_notification() {
        let config = HealthMonitorConfig::default();

        // Normal rolling average
        let rolling_avg = 100.0;

        // Latency spike detected
        let spike_latency = 300u64; // 3× average
        assert!(detect_latency_spike(spike_latency, rolling_avg, config.latency_spike_multiplier));

        // Build degradation event for latency spike
        let event = build_degradation_event(
            "provider-fast",
            "node-edge-1",
            "latency-spike",
            Some("provider-backup".to_string()),
            "initiated",
        );

        assert_eq!(event.severity, "latency-spike");
        assert_eq!(event.provider_profile_id, "provider-fast");
        assert_eq!(event.runtime_node_id, "node-edge-1");
        assert_eq!(event.fallback_route_id, Some("provider-backup".to_string()));
        assert!(!event.detected_at.is_empty());
    }

    #[test]
    fn test_health_degradation_no_fallback_available() {
        // When degraded route is last in chain, no fallback
        let chain = vec!["provider-only".to_string()];
        let fallback = select_fallback_route(&chain, "provider-only");
        assert_eq!(fallback, None);

        let event = build_degradation_event(
            "provider-only",
            "node-1",
            "unavailable",
            fallback,
            "failed",
        );
        assert_eq!(event.fallback_route_id, None);
        assert_eq!(event.pre_warm_status, "failed");
    }

    // ─── 7.9: Graceful Degradation — Services Return Errors Without Crashing ─

    #[test]
    fn test_cost_ledger_graceful_degradation_invalid_db() {
        // Open a connection to a read-only or invalid path
        // Using an in-memory DB without schema initialization simulates "unavailable"
        let conn = Connection::open_in_memory().unwrap();
        // Do NOT initialize schema — simulates corrupted/missing DB

        // Attempting to record should fail gracefully (return Err, not panic)
        let record = make_cost_record("fail-1", "agent", "chat", "2026-06-15");
        let result = record_cost_entry(&conn, &record);
        assert!(result.is_err(), "Should return error when schema is missing");

        // Attempting to query should fail gracefully
        let query = CostLedgerQuery {
            period_type: None,
            agent_id: None,
            task_type: None,
            from_date: None,
            to_date: None,
            limit: None,
        };
        let result = query_cost_dashboard(&conn, &query);
        assert!(result.is_err(), "Should return error when schema is missing");

        // Attempting projection should fail gracefully
        let result = cost_ledger_projection_from_db(&conn);
        assert!(result.is_err(), "Should return error when schema is missing");
    }

    #[test]
    fn test_federated_memory_graceful_degradation_invalid_db() {
        // Open a connection without schema initialization
        let conn = Connection::open_in_memory().unwrap();
        // Do NOT initialize schema

        // Write should fail gracefully
        let request = FactWriteRequest {
            agent_id: "strategist.core".to_string(),
            category: "system-config".to_string(),
            content: "test".to_string(),
            confidence: 0.8,
            ttl_seconds: 3600,
        };
        let result = federated_memory_write(&conn, &request);
        assert!(result.is_err(), "Write should return error when schema is missing");

        // Query should fail gracefully
        let query = FactQuery {
            category: None,
            source_agent: None,
            min_confidence: None,
            max_age_seconds: None,
            limit: None,
        };
        let result = federated_memory_query_db(&conn, "strategist.core", &query);
        assert!(result.is_err(), "Query should return error when schema is missing");

        // Read by ID should fail gracefully
        let result = federated_memory_read_by_id_db(&conn, "strategist.core", "nonexistent");
        assert!(result.is_err(), "Read should return error when schema is missing");
    }

    #[test]
    fn test_health_monitor_graceful_degradation_no_routes() {
        let config = HealthMonitorConfig::default();

        // Empty route list — should not panic
        let empty_chain: Vec<String> = Vec::new();
        let fallback = select_fallback_route(&empty_chain, "nonexistent");
        assert_eq!(fallback, None);

        // Determine health state with edge case values — should not panic
        let (state, _) = determine_health_state(true, 0, 0.0, 0, &config);
        assert_eq!(state, "ready");

        // Rolling average with empty latencies — should not panic
        let avg = compute_rolling_average(&[], config.rolling_window_size);
        assert_eq!(avg, 0.0);

        // Latency spike with zero average — should not trigger
        assert!(!detect_latency_spike(100, 0.0, config.latency_spike_multiplier));
    }

    #[test]
    fn test_all_services_error_paths_no_panic() {
        // This test verifies that all three services handle error conditions
        // without panicking, which is the core graceful degradation requirement.

        // Cost Ledger: closed connection scenario
        let conn = Connection::open_in_memory().unwrap();
        // Schema not initialized — all operations should return Err
        let record = make_cost_record("x", "a", "t", "2026-01-01");
        let _ = record_cost_entry(&conn, &record); // Should not panic
        let _ = query_cost_dashboard(
            &conn,
            &CostLedgerQuery {
                period_type: None,
                agent_id: None,
                task_type: None,
                from_date: None,
                to_date: None,
                limit: None,
            },
        ); // Should not panic
        let _ = cost_ledger_projection_from_db(&conn); // Should not panic

        // Federated Memory: schema not initialized
        let conn2 = Connection::open_in_memory().unwrap();
        let _ = federated_memory_write(
            &conn2,
            &FactWriteRequest {
                agent_id: "strategist.core".to_string(),
                category: "system-config".to_string(),
                content: "test".to_string(),
                confidence: 0.5,
                ttl_seconds: 100,
            },
        ); // Should not panic
        let _ = federated_memory_query_db(
            &conn2,
            "strategist.core",
            &FactQuery {
                category: None,
                source_agent: None,
                min_confidence: None,
                max_age_seconds: None,
                limit: None,
            },
        ); // Should not panic
        let _ = federated_memory_read_by_id_db(&conn2, "strategist.core", "x"); // Should not panic

        // Health Monitor: edge cases that could cause division by zero or overflow
        let config = HealthMonitorConfig::default();
        let _ = determine_health_state(true, u64::MAX, 0.0, 0, &config); // Should not panic
        let _ = determine_health_state(false, 0, f64::MAX, u32::MAX - 1, &config); // Should not panic
        let _ = compute_rolling_average(&[u64::MAX, u64::MAX], 10); // Should not panic
        let _ = detect_latency_spike(u64::MAX, f64::MAX, 2.0); // Should not panic

        // If we reach here, all services handled errors gracefully
    }

    #[test]
    fn test_federated_memory_access_control_graceful() {
        let conn = create_federated_memory_db();

        // Untrusted agent write — should return structured error, not panic
        let request = FactWriteRequest {
            agent_id: "untrusted.hacker".to_string(),
            category: "system-config".to_string(),
            content: "malicious data".to_string(),
            confidence: 0.9,
            ttl_seconds: 3600,
        };
        let result = federated_memory_write(&conn, &request).unwrap();
        assert!(!result.accepted);
        assert!(result.error.is_some());
        assert!(result.error.unwrap().contains("not in the Trusted_Agent_Set"));

        // Untrusted agent read — should return error, not panic
        let query = FactQuery {
            category: None,
            source_agent: None,
            min_confidence: None,
            max_age_seconds: None,
            limit: None,
        };
        let result = federated_memory_query_db(&conn, "untrusted.hacker", &query);
        assert!(result.is_err());
    }
}
