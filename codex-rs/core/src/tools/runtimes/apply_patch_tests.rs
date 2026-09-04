use super::*;
use crate::config::PermissionProfileSnapshot;
use crate::environment_selection::EnvironmentConfigOrigin;
use crate::tools::sandboxing::SandboxAttempt;
use codex_protocol::config_types::WindowsSandboxLevel;
use codex_protocol::exec_output::ExecToolCallOutput;
use codex_protocol::exec_output::StreamOutput;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::models::FileSystemPermissions;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::EnvironmentConfig;
use codex_protocol::protocol::EnvironmentConfigState;
use codex_protocol::protocol::GranularApprovalConfig;
use codex_protocol::protocol::TurnEnvironmentSelection;
use codex_sandboxing::SandboxManager;
use codex_sandboxing::SandboxType;
use codex_sandboxing::is_likely_executor_managed_sandbox_denied;
use codex_sandboxing::policy_transforms::effective_file_system_sandbox_policy;
use codex_sandboxing::policy_transforms::effective_network_sandbox_policy;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use core_test_support::PathBufExt;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
fn test_turn_environment(environment_id: &str) -> crate::session::turn_context::TurnEnvironment {
    crate::session::turn_context::TurnEnvironment::new(
        TurnEnvironmentSelection {
            environment_id: environment_id.to_string(),
            cwd: PathUri::from_abs_path(&std::env::temp_dir().abs()),
            workspace_roots: Vec::new(),
            config: EnvironmentConfigState::Ready(EnvironmentConfig {
                allow_login_shell: true,
                workspace_roots: Vec::new(),
                windows_sandbox_level: WindowsSandboxLevel::Disabled,
                windows_sandbox_private_desktop: true,
                use_legacy_landlock: false,
                permission_profile: PermissionProfileSnapshot::legacy(
                    PermissionProfile::read_only(),
                ),
                shell_environment_policy: Default::default(),
                exec_policy: None,
                mcp_policy: None,
                network_policy: None,
                selected_capability_roots: Vec::new(),
            }),
        },
        EnvironmentConfigOrigin::Thread,
        std::sync::Arc::new(codex_exec_server::Environment::default_for_tests()),
        /*shell*/ None,
    )
}

#[test]
fn wants_no_sandbox_approval_granular_respects_sandbox_flag() {
    let runtime = ApplyPatchRuntime::new();
    assert!(runtime.wants_no_sandbox_approval(AskForApproval::OnRequest));
    assert!(
        !runtime.wants_no_sandbox_approval(AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: false,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: true,
        }))
    );
    assert!(
        runtime.wants_no_sandbox_approval(AskForApproval::Granular(GranularApprovalConfig {
            sandbox_approval: true,
            rules: true,
            skill_approval: true,
            request_permissions: true,
            mcp_elicitations: true,
        }))
    );
}

#[tokio::test]
async fn approval_action_preserves_patch_path_uris() {
    let path = PathUri::parse("file:///C:/workspace/guardian-apply-patch-test.txt")
        .expect("valid foreign path URI");
    let action = ApplyPatchAction::new_add_for_test(&path, "hello".to_string());
    let expected_cwd = action.cwd.clone();
    let expected_patch = action.patch.clone();
    let request = ApplyPatchRequest {
        turn_environment: test_turn_environment(codex_exec_server::LOCAL_ENVIRONMENT_ID),
        action,
        file_paths: vec![path.clone()],
        changes: Arc::new(HashMap::new()),
        exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
    };

    let approval_action = ApplyPatchRuntime::build_approval_action(&request, "call-1");

    assert_eq!(
        approval_action,
        ApprovalAction::ApplyPatch {
            id: "call-1".to_string(),
            environment_id: codex_exec_server::LOCAL_ENVIRONMENT_ID.to_string(),
            cwd: expected_cwd,
            files: vec![path],
            patch: expected_patch,
            changes: Arc::new(HashMap::new()),
            permissions_preapproved: false,
        }
    );
}

#[tokio::test]
async fn permission_request_payload_uses_apply_patch_hook_name_and_aliases() {
    let path = std::env::temp_dir()
        .join("apply-patch-permission-request-payload.txt")
        .abs();
    let action =
        ApplyPatchAction::new_add_for_test(&PathUri::from_abs_path(&path), "hello".to_string());
    let expected_patch = action.patch.clone();
    let req = ApplyPatchRequest {
        turn_environment: test_turn_environment(codex_exec_server::LOCAL_ENVIRONMENT_ID),
        action,
        file_paths: vec![PathUri::from_abs_path(&path)],
        changes: Arc::new(HashMap::new()),
        exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
    };

    let payload =
        ApplyPatchRuntime::build_approval_action(&req, "call-1").permission_request_payload();

    assert_eq!(payload.tool_name.name(), "apply_patch");
    assert_eq!(
        payload.tool_name.matcher_aliases(),
        &["Write".to_string(), "Edit".to_string()]
    );
    assert_eq!(
        payload.tool_input,
        serde_json::json!({ "command": expected_patch })
    );
}

#[tokio::test]
async fn approval_keys_include_environment_id() {
    let runtime = ApplyPatchRuntime::new();
    let path = std::env::temp_dir()
        .join("apply-patch-approval-key.txt")
        .abs();
    let path_uri = PathUri::from_abs_path(&path);
    let req = ApplyPatchRequest {
        turn_environment: test_turn_environment("remote"),
        action: ApplyPatchAction::new_add_for_test(&path_uri, "hello".to_string()),
        file_paths: vec![path_uri.clone()],
        changes: Arc::new(HashMap::new()),
        exec_approval_requirement: ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
    };

    let keys = runtime
        .approval_action(&req, "call-1")
        .expect("build approval action")
        .cache_keys();

    assert_eq!(
        serde_json::to_value(&keys).expect("serialize approval keys"),
        serde_json::json!([
            {
                "environment_id": "remote",
                "path": path_uri,
            }
        ])
    );
}

#[tokio::test]
async fn sandbox_cwd_uses_patch_action_cwd() {
    let runtime = ApplyPatchRuntime::new();
    let path = std::env::temp_dir()
        .join("apply-patch-runtime-sandbox-cwd.txt")
        .abs();
    let req = ApplyPatchRequest {
        turn_environment: test_turn_environment(codex_exec_server::LOCAL_ENVIRONMENT_ID),
        action: ApplyPatchAction::new_add_for_test(
            &PathUri::from_abs_path(&path),
            "hello".to_string(),
        ),
        file_paths: vec![PathUri::from_abs_path(&path)],
        changes: Arc::new(HashMap::new()),
        exec_approval_requirement: ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
    };

    assert_eq!(runtime.sandbox_cwd(&req), Some(&req.action.cwd));
}

#[tokio::test]
async fn file_system_sandbox_context_preserves_executor_workspace_permissions() {
    let path = std::env::temp_dir()
        .join("apply-patch-runtime-attempt.txt")
        .abs();
    let additional_permissions = AdditionalPermissionProfile {
        network: None,
        file_system: Some(FileSystemPermissions::from_read_write_roots(
            Some(vec![path.clone()]),
            Some(Vec::new()),
        )),
    };
    let req = ApplyPatchRequest {
        turn_environment: test_turn_environment(codex_exec_server::LOCAL_ENVIRONMENT_ID),
        action: ApplyPatchAction::new_add_for_test(
            &PathUri::from_abs_path(&path),
            "hello".to_string(),
        ),
        file_paths: vec![PathUri::from_abs_path(&path)],
        changes: Arc::new(HashMap::new()),
        exec_approval_requirement: ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: Some(additional_permissions.clone()),
        permissions_preapproved: false,
    };
    let exec_server_permissions = PermissionProfile::workspace_write();
    let file_system_policy = exec_server_permissions.file_system_sandbox_policy();
    let permissions = exec_server_permissions
        .clone()
        .materialize_project_roots_with_workspace_roots(std::slice::from_ref(&path));
    let manager = SandboxManager::new();
    let sandbox_policy_cwd = PathUri::from_abs_path(&path);
    let attempt = SandboxAttempt {
        sandbox: SandboxType::MacosSeatbelt,
        sandbox_requested: true,
        permissions: &permissions,
        exec_server_permissions: &exec_server_permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &sandbox_policy_cwd,
        workspace_roots: std::slice::from_ref(&sandbox_policy_cwd),
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: true,
        windows_sandbox_level: WindowsSandboxLevel::RestrictedToken,
        windows_sandbox_private_desktop: true,
        network_denial_cancellation_token: None,
        network_proxy: None,
    };

    let sandbox = ApplyPatchRuntime::file_system_sandbox_context_for_attempt(&req, &attempt)
        .expect("sandbox context");

    let file_system_policy =
        effective_file_system_sandbox_policy(&file_system_policy, Some(&additional_permissions));
    let network_policy = effective_network_sandbox_policy(
        NetworkSandboxPolicy::Restricted,
        Some(&additional_permissions),
    );
    let expected_permissions =
        PermissionProfile::from_runtime_permissions(&file_system_policy, network_policy);
    let native_permissions: PermissionProfile = sandbox
        .permissions
        .clone()
        .try_into()
        .expect("native sandbox permissions");
    assert_eq!(native_permissions, expected_permissions);
    assert_eq!(
        sandbox.cwd,
        Some(codex_utils_path_uri::PathUri::from_abs_path(&path))
    );
    assert_eq!(
        sandbox.windows_sandbox_level,
        WindowsSandboxLevel::RestrictedToken
    );
    assert_eq!(sandbox.windows_sandbox_private_desktop, true);
    assert_eq!(sandbox.use_legacy_landlock, true);
}

#[tokio::test]
async fn file_system_sandbox_context_respects_sandbox_request() {
    let path = std::env::temp_dir()
        .join("apply-patch-runtime-none.txt")
        .abs();
    let mut req = ApplyPatchRequest {
        turn_environment: test_turn_environment(codex_exec_server::LOCAL_ENVIRONMENT_ID),
        action: ApplyPatchAction::new_add_for_test(
            &PathUri::from_abs_path(&path),
            "hello".to_string(),
        ),
        file_paths: vec![PathUri::from_abs_path(&path)],
        changes: Arc::new(HashMap::new()),
        exec_approval_requirement: ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
    };
    let permissions = PermissionProfile::Disabled;
    let manager = SandboxManager::new();
    let sandbox_policy_cwd = PathUri::from_abs_path(&path);
    let attempt = SandboxAttempt {
        sandbox: SandboxType::None,
        sandbox_requested: false,
        permissions: &permissions,
        exec_server_permissions: &permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &sandbox_policy_cwd,
        workspace_roots: std::slice::from_ref(&sandbox_policy_cwd),
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
        windows_sandbox_private_desktop: false,
        network_denial_cancellation_token: None,
        network_proxy: None,
    };

    assert_eq!(
        ApplyPatchRuntime::file_system_sandbox_context_for_attempt(&req, &attempt),
        None
    );

    let cwd = PathUri::parse("file:///C:/workspace").expect("Windows workspace URI");
    let user_home_dir = PathUri::parse("file:///C:/Users/remote").expect("Windows home URI");
    req.turn_environment.user_home_dir = Some(user_home_dir.clone());
    let permissions = PermissionProfile::workspace_write();
    let attempt = SandboxAttempt {
        sandbox_requested: true,
        permissions: &permissions,
        exec_server_permissions: &permissions,
        sandbox_cwd: &cwd,
        workspace_roots: std::slice::from_ref(&cwd),
        ..attempt
    };

    assert_eq!(
        ApplyPatchRuntime::file_system_sandbox_context_for_attempt(&req, &attempt),
        Some(FileSystemSandboxContext {
            permissions: permissions.into(),
            cwd: Some(cwd.clone()),
            workspace_roots: vec![cwd],
            user_home_dir: Some(user_home_dir),
            temporary_directories: None,
            windows_sandbox_level: WindowsSandboxLevel::RestrictedToken,
            windows_sandbox_private_desktop: false,
            windows_sandbox_proxy_settings_mode: None,
            use_legacy_landlock: false,
        })
    );
}

// --- Audit gate R2 (#22): a post-bypass failure must not escalate back to a
// sandboxed retry (which would resurface "cannot be enforced" AFTER approval).
//
// Chain being pinned, piece by piece with the real values:
//   1. after BypassSandboxFirstAttempt the attempt has sandbox_requested=false,
//      so file_system_sandbox_context_for_attempt returns None (pinned by the
//      test above) and the executor takes its existing unsandboxed path — no
//      "sandbox intent/filesystem sandbox cannot be enforced" can be produced;
//   2. if that unsandboxed execution then fails for an unrelated cause, the
//      runtime only classifies it as a sandbox denial when
//      `attempt.sandbox_requested && is_likely_executor_managed_sandbox_denied`
//      (apply_patch.rs run()): with sandbox_requested=false the classification
//      is false EVEN IF the output contains a sandbox keyword (a real EACCES
//      message contains "Permission denied"), so the orchestrator sees a
//      normal failed tool call (exit 1), not SandboxErr::Denied, and its
//      escalation branch (which is the only path that retries) never runs.
#[test]
fn r2_unrelated_failure_after_bypass_is_not_a_sandbox_denial() {
    // A REAL EACCES error, generated by the filesystem, not typed by hand:
    // the exact text a failed unsandboxed write surfaces on Termux/Linux.
    use std::os::unix::fs::PermissionsExt as _;
    struct PermRestore<'a>(&'a std::path::Path);
    impl Drop for PermRestore<'_> {
        fn drop(&mut self) {
            let _ = std::fs::set_permissions(self.0, std::fs::Permissions::from_mode(0o755));
        }
    }
    let dir = std::env::temp_dir().join("apply-patch-r2-read-only");
    std::fs::create_dir_all(&dir).expect("create dir");
    let _guard = PermRestore(&dir);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500))
        .expect("make dir read-only");
    let denied_target = dir.join("out.txt");
    let real_eacces = std::fs::write(&denied_target, b"x").expect_err("write must fail");
    println!("R2 real EACCES error = {real_eacces}");

    let output = ExecToolCallOutput {
        exit_code: 1,
        stdout: StreamOutput::new(String::new()),
        stderr: StreamOutput::new(real_eacces.to_string()),
        aggregated_output: StreamOutput::new(real_eacces.to_string()),
        duration: std::time::Duration::ZERO,
        timed_out: false,
    };

    // Premise: the matcher alone WOULD classify this output as an
    // executor-managed denial (it contains a sandbox keyword) — the danger
    // R2 guards against. Everything below talks to PRODUCTION code only:
    // no restatement of the runtime's condition exists in this test.
    assert!(
        is_likely_executor_managed_sandbox_denied(&output),
        "R2 premise: the real EACCES output does contain a sandbox keyword"
    );

    // The REAL attempt state after an approved by-construction bypass:
    // SandboxType::None with sandbox_requested=false — exactly what the
    // orchestrator produces for BypassSandboxFirstAttempt.
    let cwd = AbsolutePathBuf::from_absolute_path(&dir).expect("absolute cwd");
    let permissions = PermissionProfile::workspace_write()
        .materialize_project_roots_with_workspace_roots(std::slice::from_ref(&cwd));
    let manager = SandboxManager::new();
    let cwd_uri = PathUri::from_abs_path(&cwd);
    let attempt = SandboxAttempt {
        sandbox: SandboxType::None,
        sandbox_requested: false,
        permissions: &permissions,
        exec_server_permissions: &permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &cwd_uri,
        workspace_roots: std::slice::from_ref(&cwd_uri),
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
        windows_sandbox_private_desktop: false,
        network_denial_cancellation_token: None,
        network_proxy: None,
    };

    // Call the PRODUCTION classifier: the single copy of the runtime's
    // condition lives in apply_patch.rs. An unrelated failure after an
    // approved bypass must stay a normal failed tool call — never
    // SandboxErr::Denied, so the orchestrator's escalation branch (the
    // only path that retries) never runs.
    assert!(
        !super::classify_post_run_failure_as_sandbox_denial(
            &attempt, /*failed*/ true, &output
        ),
        "R2: an unrelated failure after an approved bypass must not be classified as a sandbox denial"
    );
}
