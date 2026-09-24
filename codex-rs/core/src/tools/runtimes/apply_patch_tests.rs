use super::*;
use crate::config::PermissionProfileSnapshot;
use crate::environment_selection::EnvironmentConfigOrigin;
use crate::sandboxing::SandboxPermissions;
use crate::tools::sandboxing::SandboxAttempt;
use crate::tools::sandboxing::SandboxOverride;
use crate::tools::sandboxing::sandbox_override_for_first_attempt;
use crate::tools::sandboxing::unsandboxed_execution_allowed;
use codex_protocol::config_types::WindowsSandboxLevel;
use codex_protocol::exec_output::ExecToolCallOutput;
use codex_protocol::exec_output::StreamOutput;
use codex_protocol::models::AdditionalPermissionProfile;
use codex_protocol::models::FileSystemPermissions;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSandboxPolicy;
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
                windows_sandbox_type: SandboxType::None,
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
        sandbox_exe: None,
        use_legacy_landlock: true,
        windows_sandbox_type: SandboxType::WindowsRestrictedToken,
        windows_sandbox_level: WindowsSandboxLevel::RestrictedToken,
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
    assert_eq!(sandbox.permissions, expected_permissions);
    assert_eq!(
        sandbox.cwd,
        codex_utils_path_uri::PathUri::from_abs_path(&path)
    );
    assert_eq!(
        sandbox.windows_sandbox_selection,
        if cfg!(windows) {
            codex_file_system::WindowsSandboxSelection::RestrictedToken
        } else {
            codex_file_system::WindowsSandboxSelection::Disabled
        }
    );
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
        sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_type: SandboxType::None,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
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
            permissions,
            cwd: cwd.clone(),
            workspace_roots: vec![cwd],
            user_home_dir: Some(user_home_dir),
            temporary_directories: None,
            windows_sandbox_selection: codex_file_system::WindowsSandboxSelection::RestrictedToken,
            windows_sandbox_proxy_settings_mode: None,
            use_legacy_landlock: false,
            sandbox_unavailable_by_construction: false,
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
        sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_type: SandboxType::None,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
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

// --- An approved apply_patch must execute without a filesystem sandbox on a
// platform that cannot provide one (Android/Termux build) ---
//
// The chain being pinned, against production code only:
//   1. the orchestrator reaches the first-attempt decision only after the
//      approval dialog returned Ok, so it passes that outcome explicitly to
//      the override (`already_approved`); the platform branch is injected
//      (`true` = the binary has no sandbox backend at all);
//   2. honoring the approval means BypassSandboxFirstAttempt, so the
//      executor-facing attempt starts with sandbox_requested=false;
//   3. with sandbox_requested=false the runtime hands the executor NO
//      filesystem sandbox context (file_system_sandbox_context_for_attempt),
//      so the executor never refuses with "filesystem sandbox cannot be
//      enforced on this executor".
#[tokio::test]
async fn approved_apply_patch_on_sandboxless_platform_executes_without_fs_sandbox() {
    let path = std::env::temp_dir()
        .join("apply-patch-approved-bypass.txt")
        .abs();
    let req = ApplyPatchRequest {
        turn_environment: test_turn_environment(codex_exec_server::LOCAL_ENVIRONMENT_ID),
        action: ApplyPatchAction::new_add_for_test(
            &PathUri::from_abs_path(&path),
            "hello".to_string(),
        ),
        file_paths: vec![PathUri::from_abs_path(&path)],
        changes: Arc::new(HashMap::new()),
        exec_approval_requirement: ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
        additional_permissions: None,
        permissions_preapproved: false,
    };

    // The same derivation the orchestrator performs on the turn environment.
    let permissions = PermissionProfile::read_only();
    let file_system_sandbox_policy = permissions.file_system_sandbox_policy();
    assert!(
        unsandboxed_execution_allowed(&file_system_sandbox_policy),
        "premise: read-only policy has no denied reads, unsandboxed execution is allowed"
    );

    let sandbox_override = sandbox_override_for_first_attempt(
        SandboxPermissions::UseDefault,
        &req.exec_approval_requirement,
        &file_system_sandbox_policy,
        /*sandbox_unavailable_by_construction*/ true,
        /*already_approved*/ true,
    );
    assert_eq!(
        sandbox_override,
        SandboxOverride::BypassSandboxFirstAttempt,
        "an approved NeedsApproval on a platform without any sandbox backend must take the unsandboxed path"
    );

    // The attempt state the orchestrator produces for
    // BypassSandboxFirstAttempt: no sandbox requested, SandboxType::None.
    let manager = SandboxManager::new();
    let cwd_uri = PathUri::from_abs_path(&path);
    let attempt = SandboxAttempt {
        sandbox: SandboxType::None,
        sandbox_requested: false,
        permissions: &permissions,
        exec_server_permissions: &permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &cwd_uri,
        workspace_roots: std::slice::from_ref(&cwd_uri),
        sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_type: SandboxType::None,
        windows_sandbox_level: WindowsSandboxLevel::Disabled,
        network_denial_cancellation_token: None,
        network_proxy: None,
    };

    assert_eq!(
        ApplyPatchRuntime::file_system_sandbox_context_for_attempt(&req, &attempt),
        None,
        "with the approved bypass no filesystem sandbox context reaches the executor, so \"filesystem sandbox cannot be enforced on this executor\" cannot be produced"
    );
}

// --- Pre-verification reads for Update/Delete under a read-only policy on a
// platform that cannot provide any filesystem sandbox (Android/Termux
// builds) ---
//
// The platform predicate is INJECTED through the sandbox context's
// `sandbox_unavailable_by_construction` flag (never `cfg!` in tests): `true`
// simulates a build where no sandbox backend can exist, `false` every other
// host. The filesystem is the production LocalFileSystem without a sandbox
// backend — what such a build provides — and the call is the production
// verification entry point the apply_patch handler uses BEFORE any execution
// or approval decision.

fn preverify_context(
    permissions: PermissionProfile,
    sandbox_unavailable_by_construction: bool,
    cwd: &PathUri,
) -> FileSystemSandboxContext {
    let mut context = FileSystemSandboxContext::from_permission_profile(permissions, cwd.clone());
    context.sandbox_unavailable_by_construction = sandbox_unavailable_by_construction;
    context
}

fn denied_read_profile(denied: &AbsolutePathBuf) -> PermissionProfile {
    let policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
        path: FileSystemPath::Path {
            path: PathUri::from_abs_path(denied),
        },
        access: FileSystemAccessMode::Deny,
        missing_path_behavior: None,
    }]);
    PermissionProfile::from_runtime_permissions(&policy, NetworkSandboxPolicy::Restricted)
}

fn parse_patch_body(patch: &str, cwd: &PathUri) -> codex_apply_patch::ApplyPatchArgs {
    let argv = vec!["apply_patch".to_string(), patch.to_string()];
    match codex_apply_patch::maybe_parse_apply_patch(&argv, cwd) {
        codex_apply_patch::MaybeApplyPatch::Body(args) => args,
        other => panic!("test patch must parse to a body, got: {other:?}"),
    }
}

#[tokio::test]
async fn delete_preverification_reads_host_files_when_no_sandbox_can_exist() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("target.txt"), "to be deleted\n").expect("write target");
    let cwd = PathUri::from_abs_path(&dir.path().to_path_buf().abs());
    let args = parse_patch_body(
        "*** Begin Patch\n*** Delete File: target.txt\n*** End Patch",
        &cwd,
    );
    let context = preverify_context(PermissionProfile::read_only(), /*platform*/ true, &cwd);
    let backend = codex_exec_server::LocalFileSystem::unsandboxed();

    let verified = codex_apply_patch::verify_apply_patch_args_with_mode(
        args,
        &cwd,
        codex_apply_patch::ApplyPatchFileUpdateMode::NormalizeToLf,
        &backend,
        Some(&context),
    )
    .await;

    let action = match verified {
        codex_apply_patch::MaybeApplyPatchVerified::Body(action) => action,
        other => panic!(
            "Delete pre-verification must read the host file when no sandbox can exist, got: {other:?}"
        ),
    };
    let target_uri = cwd.join("target.txt").expect("valid target uri");
    match action.changes().get(&target_uri) {
        Some(codex_apply_patch::ApplyPatchFileChange::Delete { content }) => {
            assert_eq!(content, "to be deleted\n");
        }
        other => panic!("expected a Delete change reading the host file, got: {other:?}"),
    }
}

#[tokio::test]
async fn update_preverification_reads_host_files_when_no_sandbox_can_exist() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("source.txt"), "alpha\nbeta\n").expect("write source");
    let cwd = PathUri::from_abs_path(&dir.path().to_path_buf().abs());
    let args = parse_patch_body(
        "*** Begin Patch\n*** Update File: source.txt\n@@\n-alpha\n+ALPHA\n*** End Patch",
        &cwd,
    );
    let context = preverify_context(PermissionProfile::read_only(), /*platform*/ true, &cwd);
    let backend = codex_exec_server::LocalFileSystem::unsandboxed();

    let verified = codex_apply_patch::verify_apply_patch_args_with_mode(
        args,
        &cwd,
        codex_apply_patch::ApplyPatchFileUpdateMode::NormalizeToLf,
        &backend,
        Some(&context),
    )
    .await;

    let action = match verified {
        codex_apply_patch::MaybeApplyPatchVerified::Body(action) => action,
        other => panic!(
            "Update pre-verification must read the host file when no sandbox can exist, got: {other:?}"
        ),
    };
    let source_uri = cwd.join("source.txt").expect("valid source uri");
    match action.changes().get(&source_uri) {
        Some(codex_apply_patch::ApplyPatchFileChange::Update { new_content, .. }) => {
            assert_eq!(new_content, "ALPHA\nbeta\n");
        }
        other => panic!("expected an Update change reading the host file, got: {other:?}"),
    }
}

#[tokio::test]
async fn denied_read_policy_keeps_preverification_fail_closed_when_no_sandbox_can_exist() {
    let dir = tempfile::tempdir().expect("tempdir");
    let denied = dir.path().join("denied.txt");
    std::fs::write(&denied, "secret\n").expect("write secret");
    let cwd = PathUri::from_abs_path(&dir.path().to_path_buf().abs());
    let args = parse_patch_body(
        "*** Begin Patch\n*** Delete File: denied.txt\n*** End Patch",
        &cwd,
    );
    // Platform without any sandbox backend, but the policy denies reads: the
    // host fallback must stay locked so the denial keeps being enforced.
    let context = preverify_context(
        denied_read_profile(&denied.abs()),
        /*platform*/ true,
        &cwd,
    );
    let backend = codex_exec_server::LocalFileSystem::unsandboxed();

    let verified = codex_apply_patch::verify_apply_patch_args_with_mode(
        args,
        &cwd,
        codex_apply_patch::ApplyPatchFileUpdateMode::NormalizeToLf,
        &backend,
        Some(&context),
    )
    .await;

    match verified {
        codex_apply_patch::MaybeApplyPatchVerified::CorrectnessError(_) => {}
        other => panic!(
            "a denied-read policy must keep pre-verification fail-closed even with no sandbox, got: {other:?}"
        ),
    }
}

#[tokio::test]
async fn linux_platform_keeps_preverification_on_the_sandboxed_routing() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("target.txt"), "to be deleted\n").expect("write target");
    let cwd = PathUri::from_abs_path(&dir.path().to_path_buf().abs());
    let args = parse_patch_body(
        "*** Begin Patch\n*** Delete File: target.txt\n*** End Patch",
        &cwd,
    );
    // Every platform that CAN provide a sandbox keeps the sandboxed routing:
    // with no sandbox backend configured this must stay an error, exactly as
    // before the read fallback existed.
    let context = preverify_context(
        PermissionProfile::read_only(),
        /*platform*/ false,
        &cwd,
    );
    let backend = codex_exec_server::LocalFileSystem::unsandboxed();

    let verified = codex_apply_patch::verify_apply_patch_args_with_mode(
        args,
        &cwd,
        codex_apply_patch::ApplyPatchFileUpdateMode::NormalizeToLf,
        &backend,
        Some(&context),
    )
    .await;

    match verified {
        codex_apply_patch::MaybeApplyPatchVerified::CorrectnessError(_) => {}
        other => panic!(
            "sandbox-capable platforms must keep the sandboxed pre-verification routing, got: {other:?}"
        ),
    }
}

#[test]
fn unapproved_apply_patch_on_sandboxless_platform_still_gets_no_bypass() {
    // The read fallback unblocks PRE-VERIFICATION only. The execution gate
    // stays: without an approval the orchestrator must not hand the executor
    // an unsandboxed attempt on a platform that cannot sandbox anything.
    let permissions = PermissionProfile::read_only();
    let file_system_sandbox_policy = permissions.file_system_sandbox_policy();
    assert!(
        unsandboxed_execution_allowed(&file_system_sandbox_policy),
        "premise: read-only policy has no denied reads"
    );

    let sandbox_override = sandbox_override_for_first_attempt(
        SandboxPermissions::UseDefault,
        &ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        },
        &file_system_sandbox_policy,
        /*sandbox_unavailable_by_construction*/ true,
        /*already_approved*/ false,
    );
    assert_ne!(
        sandbox_override,
        SandboxOverride::BypassSandboxFirstAttempt,
        "without an approval the sandboxless platform must not bypass the sandbox on the first attempt"
    );
}
