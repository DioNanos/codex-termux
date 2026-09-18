use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use pretty_assertions::assert_eq;

use super::*;

/// Debug output retains the Sediment file ID while redacting credential-bearing URLs.
#[test]
fn attachment_ref_debug_redacts_url() {
    let attachment = AttachmentRef {
        file_id: Some("sediment-file-id".to_string()),
        url: "https://attachments.test/file?signed=secret".to_string(),
    };

    assert_eq!(
        format!("{attachment:?}"),
        r#"AttachmentRef { file_id: Some("sediment-file-id"), url: "<redacted>" }"#
    );
}

/// The inline store preserves binary, text, PNG, and JPEG attachment bytes.
#[tokio::test]
async fn inline_store_preserves_attachment_bytes() {
    let cases: [(&str, &str, &[u8]); 4] = [
        (
            "attachment.bin",
            "application/octet-stream",
            b"\x00\x01\x7f\x80\xfe\xff",
        ),
        ("note.txt", "text/plain", b"hello\n"),
        ("image.png", "image/png", b"\x89PNG\r\n\x1a\n"),
        (
            "image.jpg",
            "image/jpeg",
            b"\xff\xd8\xff\xe0JFIF\x00\xff\xd9",
        ),
    ];

    for (file_name, media_type, data) in cases {
        let metadata = AttachmentMetadata {
            file_name: file_name.to_string(),
            media_type: media_type.to_string(),
        };
        let attachment = InlineAttachmentStore
            .persist(data, &metadata)
            .await
            .expect("inline attachment");
        let (data_url_metadata, encoded) =
            attachment.url.split_once(',').expect("data URL payload");
        let decoded = BASE64_STANDARD
            .decode(encoded)
            .expect("valid base64 payload");
        let expected_data_url_metadata = format!("data:{media_type};base64");

        assert_eq!(
            (attachment.file_id, data_url_metadata, decoded),
            (None, expected_data_url_metadata.as_str(), data.to_vec())
        );
    }
}
