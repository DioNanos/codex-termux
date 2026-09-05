use super::ContextualUserFragment;
use codex_protocol::models::ContentItemKind;
use codex_protocol::openai_models::ModelMessageTextTooLong;
use codex_protocol::openai_models::ModelMessages;
use codex_protocol::openai_models::validate_model_message_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardianNodeReplPolicy {
    policy: String,
}

impl GuardianNodeReplPolicy {
    pub(crate) fn from_model_messages(
        model_slug: &str,
        messages: Option<&ModelMessages>,
    ) -> Result<Self, ModelMessageTextTooLong> {
        let policy = messages
            .and_then(|messages| messages.auto_review.as_ref())
            .and_then(|messages| messages.node_repl_policy.as_deref())
            .unwrap_or(include_str!("../../assets/guardian/node_repl_policy.md"));
        validate_model_message_text(model_slug, "node_repl_policy", policy)?;
        Ok(Self {
            policy: policy.to_string(),
        })
    }
}

impl ContextualUserFragment for GuardianNodeReplPolicy {
    fn content_kind(&self) -> ContentItemKind {
        ContentItemKind("guardian.node_repl_policy".to_string())
    }

    fn role(&self) -> &'static str {
        "developer"
    }

    fn markers(&self) -> (&'static str, &'static str) {
        Self::type_markers()
    }

    fn type_markers() -> (&'static str, &'static str) {
        ("", "")
    }

    fn body(&self) -> String {
        self.policy.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guardian_node_repl_policy_rejects_oversized_values() {
        let oversized = "x".repeat(8 * 1024 + 1);
        let messages: ModelMessages = serde_json::from_value(serde_json::json!({
            "auto_review": { "node_repl_policy": oversized }
        }))
        .expect("model messages should deserialize");

        let policy = GuardianNodeReplPolicy::from_model_messages("test-model", Some(&messages))
            .expect_err("oversized node REPL policy must be rejected");
        assert!(
            policy.actual_bytes > 8 * 1024,
            "error must report the oversized node REPL policy"
        );
        assert_eq!(policy.field, "node_repl_policy");
        assert_eq!(policy.model_slug, "test-model");
    }

    #[test]
    fn guardian_node_repl_policy_accepts_none_empty_and_exact_limit() {
        assert_eq!(
            GuardianNodeReplPolicy::from_model_messages("test-model", None)
                .expect("missing policy should use the built-in")
                .body(),
            include_str!("../../assets/guardian/node_repl_policy.md")
        );

        let empty: ModelMessages = serde_json::from_value(serde_json::json!({
            "auto_review": { "node_repl_policy": "" }
        }))
        .expect("empty model messages should deserialize");
        assert_eq!(
            GuardianNodeReplPolicy::from_model_messages("test-model", Some(&empty))
                .expect("empty node REPL policy should pass")
                .body(),
            ""
        );

        let exact_text = "x".repeat(8 * 1024);
        let exact: ModelMessages = serde_json::from_value(serde_json::json!({
            "auto_review": { "node_repl_policy": exact_text }
        }))
        .expect("exact model messages should deserialize");
        let policy = GuardianNodeReplPolicy::from_model_messages("test-model", Some(&exact))
            .expect("node REPL policy at the cap should pass");
        assert_eq!(policy.body().len(), 8 * 1024);
    }
}
