use super::*;
use crate::sandboxing::SandboxPermissions;
use crate::tools::hook_names::HookToolName;
use codex_network_proxy::ManagedNetworkSandboxContext;
use codex_protocol::models::PermissionProfile;
use codex_protocol::permissions::FileSystemAccessMode;
use codex_protocol::permissions::FileSystemPath;
use codex_protocol::permissions::FileSystemSandboxEntry;
use codex_protocol::permissions::FileSystemSpecialPath;
use codex_protocol::permissions::NetworkSandboxPolicy;
use codex_protocol::protocol::GranularApprovalConfig;
use codex_sandboxing::SandboxCommand;
use codex_sandboxing::SandboxManager;
use codex_sandboxing::SandboxType;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::collections::HashMap;

#[test]
fn bash_permission_request_payload_omits_missing_description() {
    assert_eq!(
        PermissionRequestPayload::bash("echo hi".to_string(), /*description*/ None),
        PermissionRequestPayload {
            tool_name: HookToolName::bash(),
            tool_input: json!({ "command": "echo hi" }),
        }
    );
}

#[test]
fn bash_permission_request_payload_includes_description_when_present() {
    assert_eq!(
        PermissionRequestPayload::bash(
            "echo hi".to_string(),
            Some("network-access example.com".to_string()),
        ),
        PermissionRequestPayload {
            tool_name: HookToolName::bash(),
            tool_input: json!({
                "command": "echo hi",
                "description": "network-access example.com",
            }),
        }
    );
}

#[test]
fn external_sandbox_skips_exec_approval_on_request() {
    assert_eq!(
        default_exec_approval_requirement(
            AskForApproval::OnRequest,
            &FileSystemSandboxPolicy::external_sandbox(),
        ),
        ExecApprovalRequirement::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        }
    );
}

#[test]
fn restricted_sandbox_requires_exec_approval_on_request() {
    assert_eq!(
        default_exec_approval_requirement(
            AskForApproval::OnRequest,
            &FileSystemSandboxPolicy::default()
        ),
        ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        }
    );
}

#[test]
fn default_exec_approval_requirement_rejects_sandbox_prompt_when_granular_disables_it() {
    let policy = AskForApproval::Granular(GranularApprovalConfig {
        sandbox_approval: false,
        rules: true,
        skill_approval: true,
        request_permissions: true,
        mcp_elicitations: true,
    });

    let requirement =
        default_exec_approval_requirement(policy, &FileSystemSandboxPolicy::default());

    assert_eq!(
        requirement,
        ExecApprovalRequirement::Forbidden {
            reason: "approval policy disallowed sandbox approval prompt".to_string(),
        }
    );
}

#[test]
fn default_exec_approval_requirement_keeps_prompt_when_granular_allows_sandbox_approval() {
    let policy = AskForApproval::Granular(GranularApprovalConfig {
        sandbox_approval: true,
        rules: false,
        skill_approval: true,
        request_permissions: true,
        mcp_elicitations: false,
    });

    let requirement =
        default_exec_approval_requirement(policy, &FileSystemSandboxPolicy::default());

    assert_eq!(
        requirement,
        ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        }
    );
}

#[test]
fn additional_permissions_allow_bypass_sandbox_first_attempt_when_execpolicy_skips() {
    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::WithAdditionalPermissions,
            &ExecApprovalRequirement::Skip {
                bypass_sandbox: true,
                proposed_execpolicy_amendment: None,
            },
            &FileSystemSandboxPolicy::default(),
            /*sandbox_unavailable_by_construction*/ false,
        ),
        SandboxOverride::BypassSandboxFirstAttempt
    );
}

#[test]
fn guardian_bypasses_sandbox_for_explicit_escalation_on_first_attempt() {
    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::RequireEscalated,
            &ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
            &FileSystemSandboxPolicy::default(),
            /*sandbox_unavailable_by_construction*/ false,
        ),
        SandboxOverride::BypassSandboxFirstAttempt
    );
}

#[test]
fn deny_read_blocks_explicit_escalation_and_policy_bypass() {
    let file_system_policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
        path: FileSystemPath::GlobPattern {
            pattern: "**/*.env".to_string(),
        },
        access: FileSystemAccessMode::Deny,
        missing_path_behavior: None,
    }]);

    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::RequireEscalated,
            &ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
            &file_system_policy,
            /*sandbox_unavailable_by_construction*/ false,
        ),
        SandboxOverride::NoOverride,
        "explicit escalation would drop deny-read filesystem policy, so keep the first attempt sandboxed",
    );
    assert!(!unsandboxed_execution_allowed(&file_system_policy));
    assert_eq!(
        sandbox_permissions_preserving_denied_reads(
            SandboxPermissions::RequireEscalated,
            &file_system_policy,
        ),
        SandboxPermissions::UseDefault,
    );
    assert_eq!(
        sandbox_permissions_preserving_denied_reads(
            SandboxPermissions::WithAdditionalPermissions,
            &file_system_policy,
        ),
        SandboxPermissions::WithAdditionalPermissions,
    );
    assert_eq!(
        sandbox_permissions_preserving_denied_reads(
            SandboxPermissions::RequireEscalated,
            &FileSystemSandboxPolicy::default(),
        ),
        SandboxPermissions::RequireEscalated,
    );
    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::WithAdditionalPermissions,
            &ExecApprovalRequirement::Skip {
                bypass_sandbox: true,
                proposed_execpolicy_amendment: None,
            },
            &file_system_policy,
            /*sandbox_unavailable_by_construction*/ false,
        ),
        SandboxOverride::NoOverride,
        "exec-policy allow rules would drop deny-read filesystem policy, so keep the first attempt sandboxed",
    );
}

// --- Platform without sandbox by construction (Android/Termux): issue #22 ---
//
// The tests DECLARE the platform branch explicitly (they do not depend on the
// host they run on): `true` = the binary cannot contain a sandbox backend at
// all (Android/Termux build), `false` = a platform that is expected to have
// one, where a missing/broken sandbox must stay fail-closed.

#[test]
fn approved_needs_approval_bypasses_sandbox_on_platform_without_sandbox_by_construction() {
    // Termux #22: assess_patch_safety turns "no platform sandbox" into
    // AskUser; by the time the override runs the user has approved. Honoring
    // that approval means the existing unsandboxed path (no sandbox request
    // the executor would have to refuse).
    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::UseDefault,
            &ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            },
            &FileSystemSandboxPolicy::default(),
            /*sandbox_unavailable_by_construction*/ true,
        ),
        SandboxOverride::BypassSandboxFirstAttempt,
        "on a platform without any sandbox backend, an approved dialog must reach the unsandboxed path",
    );
}

#[test]
fn approved_needs_approval_keeps_sandbox_on_platform_expected_to_have_one() {
    // The negative that guards the security boundary: on Linux/macOS/Windows
    // an approved NeedsApproval does NOT drop the sandbox — the sandboxed
    // first attempt runs, and a missing/broken sandbox stays fail-closed
    // downstream (executor refuses, escalation flow handles it).
    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::UseDefault,
            &ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            },
            &FileSystemSandboxPolicy::default(),
            /*sandbox_unavailable_by_construction*/ false,
        ),
        SandboxOverride::NoOverride,
        "platforms expected to have a sandbox keep the sandboxed first attempt after approval",
    );
}

#[test]
fn by_construction_bypass_still_respects_denied_reads() {
    // The by-construction bypass is gated behind unsandboxed_execution_allowed
    // exactly like every other bypass: deny-read restrictions must survive.
    let file_system_policy = FileSystemSandboxPolicy::restricted(vec![FileSystemSandboxEntry {
        path: FileSystemPath::GlobPattern {
            pattern: "**/*.env".to_string(),
        },
        access: FileSystemAccessMode::Deny,
        missing_path_behavior: None,
    }]);
    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::UseDefault,
            &ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            },
            &file_system_policy,
            /*sandbox_unavailable_by_construction*/ true,
        ),
        SandboxOverride::NoOverride,
        "by-construction bypass would drop deny-read filesystem policy, so keep the first attempt sandboxed",
    );
}

#[test]
fn by_construction_flag_alone_does_not_bypass_without_approval() {
    // Only an APPROVED NeedsApproval bypasses: a Skip requirement on a
    // by-construction platform keeps the ordinary override semantics.
    assert_eq!(
        sandbox_override_for_first_attempt(
            SandboxPermissions::UseDefault,
            &ExecApprovalRequirement::Skip {
                bypass_sandbox: false,
                proposed_execpolicy_amendment: None,
            },
            &FileSystemSandboxPolicy::default(),
            /*sandbox_unavailable_by_construction*/ true,
        ),
        SandboxOverride::NoOverride,
        "the platform flag alone must not bypass: an approval is required",
    );
}

#[test]
fn windows_sandbox_env_preserves_denied_reads_or_rejects_unsupported_backend() {
    let temp_dir = tempfile::TempDir::new().expect("create sandbox workspace");
    let cwd = AbsolutePathBuf::from_absolute_path(
        dunce::canonicalize(temp_dir.path()).expect("canonicalize sandbox workspace"),
    )
    .expect("absolute sandbox workspace");
    let denied_path = cwd.join("blocked");
    std::fs::create_dir_all(denied_path.as_path()).expect("create denied directory");
    let denied_path = AbsolutePathBuf::from_absolute_path(
        dunce::canonicalize(denied_path.as_path()).expect("canonicalize denied directory"),
    )
    .expect("absolute denied directory");
    let file_system_policy = FileSystemSandboxPolicy::restricted(vec![
        FileSystemSandboxEntry {
            path: FileSystemPath::Special {
                value: FileSystemSpecialPath::Root,
            },
            access: FileSystemAccessMode::Read,
            missing_path_behavior: None,
        },
        FileSystemSandboxEntry {
            path: FileSystemPath::Special {
                value: FileSystemSpecialPath::project_roots(/*subpath*/ None),
            },
            access: FileSystemAccessMode::Write,
            missing_path_behavior: None,
        },
        FileSystemSandboxEntry {
            path: FileSystemPath::Path {
                path: denied_path.clone().into(),
            },
            access: FileSystemAccessMode::Deny,
            missing_path_behavior: None,
        },
    ]);
    let permissions = codex_protocol::models::PermissionProfile::from_runtime_permissions(
        &file_system_policy,
        NetworkSandboxPolicy::Restricted,
    );
    let cwd_uri = PathUri::from_abs_path(&cwd);
    let manager = SandboxManager::new();
    let mut attempt = SandboxAttempt {
        sandbox: SandboxType::WindowsRestrictedToken,
        sandbox_requested: true,
        permissions: &permissions,
        exec_server_permissions: &permissions,
        enforce_managed_network: false,
        manager: &manager,
        sandbox_cwd: &cwd_uri,
        workspace_roots: std::slice::from_ref(&cwd_uri),
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_level: codex_protocol::config_types::WindowsSandboxLevel::Elevated,
        windows_sandbox_private_desktop: false,
        network_denial_cancellation_token: None,
        network_proxy: None,
    };
    let command = || SandboxCommand {
        program: "cmd.exe".into(),
        args: vec!["/C".to_string(), "echo sandboxed".to_string()],
        cwd: cwd_uri.clone(),
        env: HashMap::new(),
        managed_network: None,
        additional_permissions: None,
    };
    let options = || crate::sandboxing::ExecOptions {
        expiration: crate::exec::ExecExpiration::DefaultTimeout,
        capture_policy: crate::exec::ExecCapturePolicy::ShellTool,
    };

    let request = attempt
        .env_for(
            command(),
            options(),
            /*network*/ None,
            /*environment_id*/ None,
        )
        .expect("prepare elevated Windows sandbox request");
    let overrides = request
        .windows_sandbox_filesystem_overrides
        .expect("elevated Windows sandbox should preserve deny-read overrides");
    assert_eq!(overrides.additional_deny_read_paths, vec![denied_path]);
    assert_eq!(request.windows_sandbox_workspace_roots, vec![cwd]);

    attempt.windows_sandbox_level =
        codex_protocol::config_types::WindowsSandboxLevel::RestrictedToken;
    let error = attempt
        .env_for(
            command(),
            options(),
            /*network*/ None,
            /*environment_id*/ None,
        )
        .expect_err("restricted-token Windows sandbox cannot enforce deny-read restrictions");
    assert_eq!(
        error.to_string(),
        "unsupported operation: windows unelevated restricted-token sandbox cannot enforce deny-read restrictions directly; refusing to run unsandboxed"
    );
}

#[test]
fn exec_server_env_keeps_command_native_and_carries_sandbox_context() {
    let cwd: AbsolutePathBuf = std::env::current_dir()
        .expect("current dir")
        .try_into()
        .expect("absolute cwd");
    let cwd_uri = PathUri::from_abs_path(&cwd);
    let exec_server_permissions = codex_protocol::models::PermissionProfile::workspace_write();
    let permissions = exec_server_permissions
        .clone()
        .materialize_project_roots_with_workspace_roots(std::slice::from_ref(&cwd));
    let manager = SandboxManager::new();
    let mut attempt = SandboxAttempt {
        sandbox: SandboxType::None,
        sandbox_requested: true,
        permissions: &permissions,
        exec_server_permissions: &exec_server_permissions,
        enforce_managed_network: true,
        manager: &manager,
        sandbox_cwd: &cwd_uri,
        workspace_roots: std::slice::from_ref(&cwd_uri),
        codex_linux_sandbox_exe: None,
        use_legacy_landlock: false,
        windows_sandbox_level: codex_protocol::config_types::WindowsSandboxLevel::Disabled,
        windows_sandbox_private_desktop: false,
        network_denial_cancellation_token: None,
        network_proxy: None,
    };
    let managed_network = ManagedNetworkSandboxContext {
        loopback_ports: vec![43123],
        allow_local_binding: false,
    };
    let command = || SandboxCommand {
        program: "/bin/bash".into(),
        args: vec!["-lc".to_string(), "pwd".to_string()],
        cwd: cwd_uri.clone(),
        env: HashMap::new(),
        managed_network: Some(managed_network.clone()),
        additional_permissions: None,
    };
    let options = || crate::sandboxing::ExecOptions {
        expiration: crate::exec::ExecExpiration::DefaultTimeout,
        capture_policy: crate::exec::ExecCapturePolicy::ShellTool,
    };
    let request = attempt
        .env_for_exec_server(command(), options())
        .expect("prepare remote exec request");

    assert_eq!(
        request.command,
        vec![
            "/bin/bash".to_string(),
            "-lc".to_string(),
            "pwd".to_string()
        ]
    );
    assert_eq!(request.arg0, None);
    assert_eq!(request.sandbox, SandboxType::None);
    assert_eq!(
        request.exec_server_sandbox,
        Some(codex_exec_server::FileSystemSandboxContext {
            permissions: exec_server_permissions.clone().into(),
            cwd: Some(cwd_uri.clone()),
            workspace_roots: vec![cwd_uri.clone()],
            windows_sandbox_level: if cfg!(windows) {
                codex_protocol::config_types::WindowsSandboxLevel::RestrictedToken
            } else {
                codex_protocol::config_types::WindowsSandboxLevel::Disabled
            },
            windows_sandbox_private_desktop: false,
            windows_sandbox_proxy_settings_mode: None,
            use_legacy_landlock: false,
        })
    );
    assert!(request.exec_server_enforce_managed_network);
    assert_eq!(
        request.exec_server_managed_network,
        Some(managed_network.clone())
    );

    attempt.sandbox_requested = false;
    let request = attempt
        .env_for_exec_server(command(), options())
        .expect("prepare unsandboxed remote exec request");

    assert_eq!(request.exec_server_sandbox, None);
    assert!(!request.exec_server_enforce_managed_network);
    assert_eq!(request.exec_server_managed_network, Some(managed_network));
}

// --- Audit gate R1 (#22): the REAL default configuration, not a fixture ---

#[test]
fn r1_default_configuration_allows_the_approved_unsandboxed_path() {
    // The CLI defaults when the user configures nothing (the Termux npm
    // package ships no config of its own): a workspace-write permission
    // profile — projected exactly the way the session does it for the turn
    // environment — and the `#[default] OnRequest` approval policy. Measure
    // the gate the approved-unsandboxed path depends on.
    let cwd =
        AbsolutePathBuf::try_from(std::env::temp_dir().as_path()).expect("temp dir is absolute");
    let profile = PermissionProfile::workspace_write()
        .materialize_project_roots_with_workspace_roots(std::slice::from_ref(&cwd));
    let fs_policy = profile.file_system_sandbox_policy();
    let approval = AskForApproval::default();
    println!("R1 permission_profile = workspace_write (default CLI sandbox policy)");
    println!("R1 fs_policy.kind = {:?}", fs_policy.kind);
    println!(
        "R1 fs_policy.has_denied_read_restrictions = {}",
        fs_policy.has_denied_read_restrictions()
    );
    println!(
        "R1 unsandboxed_execution_allowed = {}",
        unsandboxed_execution_allowed(&fs_policy)
    );
    println!("R1 default AskForApproval = {approval:?} (not sandbox-rejecting)");
    assert!(
        unsandboxed_execution_allowed(&fs_policy),
        "R1: the default configuration must allow the approved unsandboxed path — otherwise the fix does not close the issue"
    );
}
