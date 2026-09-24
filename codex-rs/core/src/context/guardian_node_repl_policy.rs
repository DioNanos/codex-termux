use super::ContextualUserFragment;
use codex_prompts::ResolvedModelMessages;
use codex_protocol::models::ContentItemKind;
use codex_protocol::openai_models::ModelMessageTextTooLong;
use codex_protocol::openai_models::validate_model_message_text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GuardianNodeReplPolicy {
    policy: String,
}

impl GuardianNodeReplPolicy {
    pub(crate) fn from_messages(
        model_slug: &str,
        model_messages: ResolvedModelMessages<'_>,
    ) -> Result<Self, ModelMessageTextTooLong> {
        let policy = model_messages.auto_review().node_repl_policy;
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

    fn model_info_with_messages(
        messages: serde_json::Value,
    ) -> codex_protocol::openai_models::ModelInfo {
        let mut model = codex_models_manager::model_info::model_info_from_slug("test-model");
        model.model_messages =
            Some(serde_json::from_value(messages).expect("model messages should deserialize"));
        model
    }

    #[test]
    fn guardian_node_repl_policy_rejects_oversized_values() {
        let oversized = "x".repeat(8 * 1024 + 1);
        let model = model_info_with_messages(serde_json::json!({
            "auto_review": { "node_repl_policy": oversized }
        }));

        let policy = GuardianNodeReplPolicy::from_messages(
            model.slug.as_str(),
            ResolvedModelMessages::from_model(&model),
        )
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
            GuardianNodeReplPolicy::from_messages("test-model", ResolvedModelMessages::bundled())
                .expect("missing policy should use the built-in")
                .body(),
            codex_prompts::ResolvedModelMessages::bundled()
                .auto_review()
                .node_repl_policy
        );

        let empty = model_info_with_messages(serde_json::json!({
            "auto_review": { "node_repl_policy": "" }
        }));
        assert_eq!(
            GuardianNodeReplPolicy::from_messages(
                "test-model",
                ResolvedModelMessages::from_model(&empty),
            )
            .expect("empty node REPL policy should pass")
            .body(),
            ""
        );

        let exact_text = "x".repeat(8 * 1024);
        let exact = model_info_with_messages(serde_json::json!({
            "auto_review": { "node_repl_policy": exact_text }
        }));
        let policy = GuardianNodeReplPolicy::from_messages(
            "test-model",
            ResolvedModelMessages::from_model(&exact),
        )
        .expect("node REPL policy at the cap should pass");
        assert_eq!(policy.body().len(), 8 * 1024);
    }
}
