use super::take_shutdown_request;

#[test]
fn inherited_control_path_cannot_consume_another_process_request() {
    let directory = tempfile::tempdir().expect("directory");
    let request = directory.path().join("shutdown");
    std::fs::write(&request, "1234").expect("request");
    assert!(!take_shutdown_request(&request, /*pid*/ 5678).expect("descendant probe"));
    assert!(take_shutdown_request(&request, /*pid*/ 1234).expect("parent probe"));
    assert!(!take_shutdown_request(&request, /*pid*/ 1234).expect("already consumed"));
}

#[test]
fn incomplete_or_invalid_requests_are_not_consumed() {
    let directory = tempfile::tempdir().expect("directory");
    let request = directory.path().join("shutdown");
    for contents in [b"".as_slice(), b"123", b"1234junk", &[0xff; 20]] {
        std::fs::write(&request, contents).expect("request");
        assert!(!take_shutdown_request(&request, /*pid*/ 1234).expect("probe"));
        assert!(request.exists());
    }
}
