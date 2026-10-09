use super::is_valid_se_process_context;
use super::se_process_context_from_proc;
use pretty_assertions::assert_eq;

#[test]
fn accepts_contexts_with_zero_two_and_four_categories() {
    for label in [
        "u:r:untrusted_app:s0",
        "u:r:untrusted_app_27:s0:c209,c256",
        "u:r:untrusted_app:s0:c209,c256,c512,c768",
        "u:r:su:s0",
        "u:r:app.domain-x:s0:c0,c1",
    ] {
        assert!(
            is_valid_se_process_context(label),
            "{label:?} must be accepted"
        );
    }
}

#[test]
fn rejects_labels_that_are_not_android_process_contexts() {
    for label in [
        // An AppArmor label, as seen on non-Android Linux kernels.
        "unconfined",
        "/usr/bin/foo (enforce)",
        "",
        " ",
        "u:r:untrusted app:s0",
        "u:r:untrusted_app:s0 ",
        " u:r:untrusted_app:s0",
        "u:r:untrusted_app:s0\n",
        "u:r::s0",
        "u:object_r:untrusted_app:s0",
        "u:r:untrusted_app:s1",
        "u:r:untrusted_app",
        "u:r:untrusted_app:s0:",
    ] {
        assert!(
            !is_valid_se_process_context(label),
            "{label:?} must be rejected"
        );
    }
}

#[test]
fn rejects_malformed_category_lists() {
    for label in [
        "u:r:untrusted_app:s0:c1",
        "u:r:untrusted_app:s0:c1,c2,c3",
        "u:r:untrusted_app:s0:c1,c2,c3,c4,c5",
        "u:r:untrusted_app:s0:c,c2",
        "u:r:untrusted_app:s0:c1,x2",
        "u:r:untrusted_app:s0:1,2",
        "u:r:untrusted_app:s0:c1,c2,",
        "u:r:untrusted_app:s0:c1,,c2",
        "u:r:untrusted_app:s0:c1,c2 ",
        "u:r:untrusted_app:s0,c1,c2",
    ] {
        assert!(
            !is_valid_se_process_context(label),
            "{label:?} must be rejected"
        );
    }
}

#[test]
fn proc_contents_lose_trailing_nul_and_newline_before_validation() {
    assert_eq!(
        se_process_context_from_proc(b"u:r:untrusted_app:s0:c209,c256\0"),
        Some("u:r:untrusted_app:s0:c209,c256")
    );
    assert_eq!(
        se_process_context_from_proc(b"u:r:untrusted_app:s0\n\0"),
        Some("u:r:untrusted_app:s0")
    );
    assert_eq!(
        se_process_context_from_proc(b"u:r:untrusted_app:s0"),
        Some("u:r:untrusted_app:s0")
    );
}

#[test]
fn proc_contents_that_are_not_a_context_are_ignored() {
    assert_eq!(se_process_context_from_proc(b""), None);
    assert_eq!(se_process_context_from_proc(b"\0"), None);
    assert_eq!(se_process_context_from_proc(b"unconfined\n"), None);
    assert_eq!(se_process_context_from_proc(b"u:r:app\xff:s0"), None);
    assert_eq!(
        se_process_context_from_proc(b"u:r:untrusted_app:s0\0u:r:other:s0"),
        None
    );
}
