// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.4, 3.5
// Sensitivity Classifier — classifies prompts as Sensitive or NonSensitive

use crate::mesh::trust::PromptSensitivity;
use serde::{Deserialize, Serialize};

// ─── Classification Result ───────────────────────────────────────────────────

/// Result of classifying a prompt's sensitivity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationResult {
    pub sensitivity: PromptSensitivity,
    pub confidence: f64,
    pub method: ClassificationMethod,
    pub reasons: Vec<String>,
}

/// How the classification was determined.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClassificationMethod {
    /// User marked conversation as private.
    UserExplicit,
    /// Matched a sensitive keyword.
    KeywordMatch,
    /// Conversation context suggests sensitive.
    ContextBased,
    /// No signals, using default policy.
    DefaultPolicy,
    /// User explicitly overrode classification for this message.
    UserOverride,
}

// ─── Configuration ───────────────────────────────────────────────────────────

/// Configuration for the sensitivity classifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityConfig {
    /// Default policy when no signals match (default: NonSensitive).
    pub default_policy: PromptSensitivity,
    /// Keywords that trigger Sensitive classification (case-insensitive).
    pub sensitive_keywords: Vec<String>,
    /// Agent personas that imply sensitivity.
    pub sensitive_personas: Vec<String>,
    /// Regex patterns for private code repositories.
    pub private_repo_patterns: Vec<String>,
}

impl Default for SensitivityConfig {
    fn default() -> Self {
        Self {
            default_policy: PromptSensitivity::NonSensitive,
            sensitive_keywords: vec![
                "password".to_string(),
                "secret".to_string(),
                "api_key".to_string(),
                "private_key".to_string(),
                "credit_card".to_string(),
                "ssn".to_string(),
                "social_security".to_string(),
                "bank_account".to_string(),
                "medical".to_string(),
                "diagnosis".to_string(),
            ],
            sensitive_personas: vec![
                "medical_assistant".to_string(),
                "financial_advisor".to_string(),
                "legal_counsel".to_string(),
            ],
            private_repo_patterns: vec![
                r"private[-_]".to_string(),
                r"internal[-_]".to_string(),
                r"confidential".to_string(),
            ],
        }
    }
}

// ─── Conversation Context ────────────────────────────────────────────────────

/// Context about the current conversation that influences classification.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationContext {
    /// User has marked this conversation as private.
    pub user_marked_private: bool,
    /// User has explicitly allowed mesh routing.
    pub user_marked_public: bool,
    /// The active persona/agent name.
    pub persona: Option<String>,
    /// Repository references in the conversation.
    pub repo_references: Vec<String>,
}

// ─── Sensitivity Classifier ──────────────────────────────────────────────────

/// Classifies prompts by sensitivity level to determine routing constraints.
/// Priority order: UserOverride > UserExplicit > KeywordMatch > ContextBased > DefaultPolicy
pub struct SensitivityClassifier {
    config: SensitivityConfig,
    /// Per-message override (set by user, cleared after use).
    current_override: Option<PromptSensitivity>,
}

impl SensitivityClassifier {
    pub fn new(config: SensitivityConfig) -> Self {
        Self {
            config,
            current_override: None,
        }
    }

    /// Set a per-message override that bypasses all other classification.
    pub fn set_override(&mut self, sensitivity: PromptSensitivity) {
        self.current_override = Some(sensitivity);
    }

    /// Clear the per-message override.
    pub fn clear_override(&mut self) {
        self.current_override = None;
    }

    /// Classify a prompt's sensitivity.
    /// Every prompt is guaranteed to receive a classification (never returns None).
    pub fn classify(
        &mut self,
        prompt: &str,
        context: &ConversationContext,
    ) -> ClassificationResult {
        // Priority 1: Per-message user override
        if let Some(override_sensitivity) = self.current_override.take() {
            return ClassificationResult {
                sensitivity: override_sensitivity,
                confidence: 1.0,
                method: ClassificationMethod::UserOverride,
                reasons: vec!["User explicitly overrode classification for this message".to_string()],
            };
        }

        // Priority 2: Conversation-level privacy flag
        if context.user_marked_private {
            return ClassificationResult {
                sensitivity: PromptSensitivity::Sensitive,
                confidence: 1.0,
                method: ClassificationMethod::UserExplicit,
                reasons: vec!["User marked conversation as private".to_string()],
            };
        }

        if context.user_marked_public {
            return ClassificationResult {
                sensitivity: PromptSensitivity::NonSensitive,
                confidence: 1.0,
                method: ClassificationMethod::UserExplicit,
                reasons: vec!["User explicitly allowed mesh routing".to_string()],
            };
        }

        // Priority 3: Keyword matching (case-insensitive)
        let prompt_lower = prompt.to_lowercase();
        let matched_keywords: Vec<String> = self
            .config
            .sensitive_keywords
            .iter()
            .filter(|kw| prompt_lower.contains(&kw.to_lowercase()))
            .cloned()
            .collect();

        if !matched_keywords.is_empty() {
            return ClassificationResult {
                sensitivity: PromptSensitivity::Sensitive,
                confidence: 0.9,
                method: ClassificationMethod::KeywordMatch,
                reasons: matched_keywords
                    .iter()
                    .map(|kw| format!("Contains sensitive keyword: {}", kw))
                    .collect(),
            };
        }

        // Priority 4: Context-based classification
        // Check persona
        if let Some(ref persona) = context.persona {
            let persona_lower = persona.to_lowercase();
            if self
                .config
                .sensitive_personas
                .iter()
                .any(|p| persona_lower.contains(&p.to_lowercase()))
            {
                return ClassificationResult {
                    sensitivity: PromptSensitivity::Sensitive,
                    confidence: 0.8,
                    method: ClassificationMethod::ContextBased,
                    reasons: vec![format!(
                        "Persona '{}' implies sensitive content",
                        persona
                    )],
                };
            }
        }

        // Check private repo patterns
        for repo_ref in &context.repo_references {
            let repo_lower = repo_ref.to_lowercase();
            if self
                .config
                .private_repo_patterns
                .iter()
                .any(|pattern| repo_lower.contains(&pattern.to_lowercase()))
            {
                return ClassificationResult {
                    sensitivity: PromptSensitivity::Sensitive,
                    confidence: 0.85,
                    method: ClassificationMethod::ContextBased,
                    reasons: vec!["References private repository code".to_string()],
                };
            }
        }

        // Priority 5: Default policy
        ClassificationResult {
            sensitivity: self.config.default_policy,
            confidence: 0.5,
            method: ClassificationMethod::DefaultPolicy,
            reasons: vec!["No sensitivity signals detected, using default policy".to_string()],
        }
    }

    /// Get the current configuration.
    pub fn config(&self) -> &SensitivityConfig {
        &self.config
    }

    /// Update the configuration.
    pub fn update_config(&mut self, config: SensitivityConfig) {
        self.config = config;
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn make_classifier() -> SensitivityClassifier {
        SensitivityClassifier::new(SensitivityConfig::default())
    }

    fn arb_prompt() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9 ]{1,200}"
    }

    proptest! {
        /// Property: user-explicit always takes priority over keywords and context.
        #[test]
        fn prop_user_explicit_takes_priority(
            prompt in arb_prompt()
        ) {
            let mut classifier = make_classifier();
            let context = ConversationContext {
                user_marked_private: true,
                ..Default::default()
            };

            let result = classifier.classify(&prompt, &context);
            prop_assert_eq!(result.sensitivity, PromptSensitivity::Sensitive);
            prop_assert_eq!(result.method, ClassificationMethod::UserExplicit);
            prop_assert_eq!(result.confidence, 1.0);
        }

        /// Property: keyword match always returns Sensitive.
        #[test]
        fn prop_keyword_match_always_sensitive(
            prefix in "[a-z]{0,20}",
            suffix in "[a-z]{0,20}"
        ) {
            let mut classifier = make_classifier();
            let context = ConversationContext::default();

            // Construct a prompt containing a sensitive keyword
            let prompt = format!("{} password {}", prefix, suffix);
            let result = classifier.classify(&prompt, &context);
            prop_assert_eq!(result.sensitivity, PromptSensitivity::Sensitive);
            prop_assert_eq!(result.method, ClassificationMethod::KeywordMatch);
        }

        /// Property: unclassified prompts default to configured policy.
        #[test]
        fn prop_unclassified_defaults_to_policy(
            prompt in "[a-z]{5,50}"  // Only lowercase letters, no keywords
        ) {
            let mut classifier = make_classifier();
            let context = ConversationContext::default();

            // Ensure prompt doesn't contain any sensitive keywords
            let has_keyword = classifier.config().sensitive_keywords.iter().any(|kw| {
                prompt.to_lowercase().contains(&kw.to_lowercase())
            });

            if !has_keyword {
                let result = classifier.classify(&prompt, &context);
                prop_assert_eq!(result.sensitivity, PromptSensitivity::NonSensitive);
                prop_assert_eq!(result.method, ClassificationMethod::DefaultPolicy);
            }
        }

        /// Property: every prompt gets classified (no None results).
        /// This is guaranteed by the return type, but we verify the method is always set.
        #[test]
        fn prop_every_prompt_classified(
            prompt in arb_prompt(),
            private in any::<bool>(),
            public in any::<bool>()
        ) {
            let mut classifier = make_classifier();
            let context = ConversationContext {
                user_marked_private: private && !public,
                user_marked_public: public && !private,
                ..Default::default()
            };

            let result = classifier.classify(&prompt, &context);
            // Every result has a valid method
            prop_assert!(matches!(
                result.method,
                ClassificationMethod::UserExplicit
                    | ClassificationMethod::UserOverride
                    | ClassificationMethod::KeywordMatch
                    | ClassificationMethod::ContextBased
                    | ClassificationMethod::DefaultPolicy
            ));
            // Confidence is always in valid range
            prop_assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
            // Reasons are never empty
            prop_assert!(!result.reasons.is_empty());
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_user_override_bypasses_all() {
        let mut classifier = make_classifier();
        classifier.set_override(PromptSensitivity::NonSensitive);

        // Even with a keyword-laden prompt and private context
        let context = ConversationContext {
            user_marked_private: true,
            ..Default::default()
        };
        let result = classifier.classify("my password is secret", &context);
        assert_eq!(result.method, ClassificationMethod::UserOverride);
        assert_eq!(result.sensitivity, PromptSensitivity::NonSensitive);
    }

    #[test]
    fn test_override_consumed_after_use() {
        let mut classifier = make_classifier();
        classifier.set_override(PromptSensitivity::Sensitive);

        let context = ConversationContext::default();
        let r1 = classifier.classify("hello", &context);
        assert_eq!(r1.method, ClassificationMethod::UserOverride);

        // Second call should use normal classification
        let r2 = classifier.classify("hello", &context);
        assert_eq!(r2.method, ClassificationMethod::DefaultPolicy);
    }

    #[test]
    fn test_persona_triggers_sensitive() {
        let mut classifier = make_classifier();
        let context = ConversationContext {
            persona: Some("medical_assistant".to_string()),
            ..Default::default()
        };

        let result = classifier.classify("what are my symptoms", &context);
        assert_eq!(result.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(result.method, ClassificationMethod::ContextBased);
    }

    #[test]
    fn test_private_repo_triggers_sensitive() {
        let mut classifier = make_classifier();
        let context = ConversationContext {
            repo_references: vec!["private-company-api".to_string()],
            ..Default::default()
        };

        let result = classifier.classify("show me the code", &context);
        assert_eq!(result.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(result.method, ClassificationMethod::ContextBased);
    }

    #[test]
    fn test_case_insensitive_keyword_match() {
        let mut classifier = make_classifier();
        let context = ConversationContext::default();

        let result = classifier.classify("What is my PASSWORD?", &context);
        assert_eq!(result.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(result.method, ClassificationMethod::KeywordMatch);
    }

    #[test]
    fn test_custom_config_default_policy() {
        let config = SensitivityConfig {
            default_policy: PromptSensitivity::Sensitive,
            sensitive_keywords: vec![],
            sensitive_personas: vec![],
            private_repo_patterns: vec![],
        };
        let mut classifier = SensitivityClassifier::new(config);
        let context = ConversationContext::default();

        let result = classifier.classify("hello world", &context);
        assert_eq!(result.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(result.method, ClassificationMethod::DefaultPolicy);
    }
}
