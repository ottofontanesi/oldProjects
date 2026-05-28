//! Tool Call Sanitizer
//!
//! Removes or masks sensitive values (API keys, tokens, passwords, credentials)
//! from tool call input parameters before logging. Runs synchronously in the
//! interceptor before the record enters the channel.

use once_cell::sync::Lazy;
use regex::RegexSet;
use serde_json::Value;

// ─── Secret Deny-List and Patterns ──────────────────────────────────────────

/// Default deny-list for parameter names that indicate secrets.
/// Matching is case-insensitive.
pub const SECRET_PARAM_NAMES: &[&str] = &[
    "password",
    "secret",
    "token",
    "api_key",
    "apikey",
    "apiKey",
    "authorization",
    "private_key",
    "credentials",
    "connection_string",
    "api_secret",
    "access_token",
    "refresh_token",
];

/// Regex patterns for detecting secret values regardless of parameter name.
static SECRET_VALUE_REGEX_SET: Lazy<RegexSet> = Lazy::new(|| {
    RegexSet::new(&[
        r"^Bearer\s+.+",                                          // Bearer tokens
        r"^sk-[a-zA-Z0-9]{20,}",                                 // OpenAI-style keys
        r"^pk-[a-zA-Z0-9]{20,}",                                 // Public keys with pk- prefix
        r"^eyJ[a-zA-Z0-9_-]+\.[a-zA-Z0-9_-]+\.[a-zA-Z0-9_-]+", // JWT tokens
        r"^[A-Za-z0-9+/]{32,}={0,2}$",                           // Base64 keys > 32 chars
        r"-----BEGIN\s+(RSA\s+)?PRIVATE\s+KEY-----",              // PEM private keys
    ])
    .expect("Failed to compile secret value regex set")
});

/// The redaction placeholder.
const REDACTED: &str = "[REDACTED]";

// ─── Public API ─────────────────────────────────────────────────────────────

/// Check if a parameter name matches the secret deny-list (case-insensitive).
pub fn is_secret_param_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    SECRET_PARAM_NAMES
        .iter()
        .any(|secret_name| lower == secret_name.to_lowercase())
}

/// Check if a value matches any secret value regex pattern.
pub fn is_secret_value(value: &str) -> bool {
    SECRET_VALUE_REGEX_SET.is_match(value)
}

/// Sanitize input parameters, replacing secret values with "[REDACTED]".
/// Runs synchronously in the interceptor before the record enters the channel.
///
/// Rules:
/// 1. If params is an object, check each key against SECRET_PARAM_NAMES
/// 2. For each string value (regardless of key), check against SECRET_VALUE_PATTERNS
/// 3. Replace matching values with Value::String("[REDACTED]")
/// 4. Recurse into nested objects and arrays
/// 5. Default-open: preserve values that don't match any pattern
pub fn sanitize_parameters(params: &Value) -> Value {
    match params {
        Value::Object(map) => {
            let mut sanitized_map = serde_json::Map::new();
            for (key, value) in map {
                if is_secret_param_name(key) {
                    // Redact the entire value for secret-named params
                    sanitized_map.insert(key.clone(), Value::String(REDACTED.to_string()));
                } else {
                    sanitized_map.insert(key.clone(), sanitize_value(value));
                }
            }
            Value::Object(sanitized_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| sanitize_value(v)).collect())
        }
        _ => sanitize_value(params),
    }
}

/// Sanitize a single value, checking string values against patterns and recursing
/// into nested structures.
fn sanitize_value(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            if is_secret_value(s) {
                Value::String(REDACTED.to_string())
            } else {
                Value::String(s.clone())
            }
        }
        Value::Object(map) => {
            let mut sanitized_map = serde_json::Map::new();
            for (key, val) in map {
                if is_secret_param_name(key) {
                    sanitized_map.insert(key.clone(), Value::String(REDACTED.to_string()));
                } else {
                    sanitized_map.insert(key.clone(), sanitize_value(val));
                }
            }
            Value::Object(sanitized_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| sanitize_value(v)).collect())
        }
        // Numbers, booleans, null — pass through unchanged
        other => other.clone(),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use serde_json::json;

    // ─── Unit Tests ─────────────────────────────────────────────────────────

    #[test]
    fn test_is_secret_param_name_case_insensitive() {
        assert!(is_secret_param_name("password"));
        assert!(is_secret_param_name("PASSWORD"));
        assert!(is_secret_param_name("Password"));
        assert!(is_secret_param_name("api_key"));
        assert!(is_secret_param_name("API_KEY"));
        assert!(is_secret_param_name("apiKey"));
        assert!(is_secret_param_name("APIKEY"));
        assert!(is_secret_param_name("token"));
        assert!(is_secret_param_name("authorization"));
        assert!(!is_secret_param_name("username"));
        assert!(!is_secret_param_name("path"));
        assert!(!is_secret_param_name("file_name"));
    }

    #[test]
    fn test_is_secret_value_bearer() {
        assert!(is_secret_value("Bearer eyJhbGciOiJIUzI1NiJ9.payload.sig"));
        assert!(!is_secret_value("bearer")); // no space + content
    }

    #[test]
    fn test_is_secret_value_openai_key() {
        assert!(is_secret_value("sk-abcdefghijklmnopqrstuvwxyz1234567890"));
        assert!(!is_secret_value("sk-short")); // too short
    }

    #[test]
    fn test_is_secret_value_jwt() {
        assert!(is_secret_value("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U"));
    }

    #[test]
    fn test_is_secret_value_pem() {
        assert!(is_secret_value("-----BEGIN PRIVATE KEY-----"));
        assert!(is_secret_value("-----BEGIN RSA PRIVATE KEY-----"));
    }

    #[test]
    fn test_is_secret_value_base64_long() {
        let long_b64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnop==";
        assert!(is_secret_value(long_b64));
    }

    #[test]
    fn test_sanitize_simple_object() {
        let input = json!({
            "path": "src/main.rs",
            "password": "super_secret_123",
            "content": "hello world"
        });

        let result = sanitize_parameters(&input);
        assert_eq!(result["path"], "src/main.rs");
        assert_eq!(result["password"], "[REDACTED]");
        assert_eq!(result["content"], "hello world");
    }

    #[test]
    fn test_sanitize_nested_object() {
        let input = json!({
            "config": {
                "api_key": "sk-abcdefghijklmnopqrstuvwxyz1234567890",
                "host": "localhost"
            },
            "data": "safe"
        });

        let result = sanitize_parameters(&input);
        assert_eq!(result["config"]["api_key"], "[REDACTED]");
        assert_eq!(result["config"]["host"], "localhost");
        assert_eq!(result["data"], "safe");
    }

    #[test]
    fn test_sanitize_value_pattern_match() {
        let input = json!({
            "header": "Bearer eyJhbGciOiJIUzI1NiJ9.payload.signature",
            "name": "test"
        });

        let result = sanitize_parameters(&input);
        assert_eq!(result["header"], "[REDACTED]");
        assert_eq!(result["name"], "test");
    }

    #[test]
    fn test_sanitize_array_with_secrets() {
        let input = json!({
            "tokens": ["sk-abcdefghijklmnopqrstuvwxyz1234567890", "safe_value"],
            "names": ["alice", "bob"]
        });

        let result = sanitize_parameters(&input);
        assert_eq!(result["tokens"][0], "[REDACTED]");
        assert_eq!(result["tokens"][1], "safe_value");
        assert_eq!(result["names"][0], "alice");
    }

    #[test]
    fn test_sanitize_deeply_nested() {
        let input = json!({
            "level1": {
                "level2": {
                    "level3": {
                        "secret": "my_password",
                        "safe": "visible"
                    }
                }
            }
        });

        let result = sanitize_parameters(&input);
        assert_eq!(result["level1"]["level2"]["level3"]["secret"], "[REDACTED]");
        assert_eq!(result["level1"]["level2"]["level3"]["safe"], "visible");
    }

    // ─── Property-Based Tests (Task 2.4) ────────────────────────────────────

    // Feature: tool-call-tracker, Property 3: Secret sanitization completeness
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 2.1, 2.2, 2.3**
        #[test]
        fn prop_sanitization_completeness(
            secret_name_idx in 0usize..13,
            safe_key in "[a-z]{3,10}",
            safe_value in "[a-z0-9 ]{1,20}",
        ) {
            let secret_name = SECRET_PARAM_NAMES[secret_name_idx];
            let secret_value = "super_secret_password_123";

            // Build a JSON object with a secret-named param
            let input = json!({
                secret_name: secret_value,
                safe_key.clone(): safe_value.clone(),
            });

            let result = sanitize_parameters(&input);

            // The secret-named param must be redacted
            prop_assert_eq!(
                result[secret_name].as_str().unwrap(),
                "[REDACTED]"
            );

            // Structure preserved: same keys exist
            prop_assert!(result.as_object().unwrap().contains_key(secret_name));
            prop_assert!(result.as_object().unwrap().contains_key(&safe_key));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property 3 continued: secret value patterns are redacted regardless of key name
        /// **Validates: Requirements 2.1, 2.2, 2.3**
        #[test]
        fn prop_sanitization_value_patterns(
            key_name in "[a-z_]{3,15}",
        ) {
            // Test with various secret value patterns
            let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
            let bearer = "Bearer some-long-token-value-here";
            let sk_key = "sk-abcdefghijklmnopqrstuvwxyz1234567890";

            let input_jwt = json!({ key_name.clone(): jwt });
            let input_bearer = json!({ key_name.clone(): bearer });
            let input_sk = json!({ key_name.clone(): sk_key });

            let result_jwt = sanitize_parameters(&input_jwt);
            let result_bearer = sanitize_parameters(&input_bearer);
            let result_sk = sanitize_parameters(&input_sk);

            prop_assert_eq!(result_jwt[&key_name].as_str().unwrap(), "[REDACTED]");
            prop_assert_eq!(result_bearer[&key_name].as_str().unwrap(), "[REDACTED]");
            prop_assert_eq!(result_sk[&key_name].as_str().unwrap(), "[REDACTED]");

            // Structure preserved
            prop_assert!(result_jwt.as_object().unwrap().contains_key(&key_name));
        }
    }

    // Feature: tool-call-tracker, Property 4: Sanitizer preserves non-secret values
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 2.5**
        #[test]
        fn prop_non_secret_preservation(
            key1 in "path|file|name|host|port|count|mode|format|encoding",
            val1 in "[a-z0-9./_ -]{1,30}",
            key2 in "url|dir|output|input|source|target|label",
            val2 in "[a-z0-9./_ -]{1,30}",
            num_val in 0i64..10000,
            bool_val in proptest::bool::ANY,
        ) {
            let input = json!({
                key1.clone(): val1.clone(),
                key2.clone(): val2.clone(),
                "number_field": num_val,
                "bool_field": bool_val,
            });

            let result = sanitize_parameters(&input);

            // All non-secret values should be preserved exactly
            prop_assert_eq!(result[&key1].as_str().unwrap(), val1.as_str());
            prop_assert_eq!(result[&key2].as_str().unwrap(), val2.as_str());
            prop_assert_eq!(result["number_field"].as_i64().unwrap(), num_val);
            prop_assert_eq!(result["bool_field"].as_bool().unwrap(), bool_val);

            // Structure is identical
            prop_assert_eq!(
                result.as_object().unwrap().len(),
                input.as_object().unwrap().len()
            );
        }
    }
}
