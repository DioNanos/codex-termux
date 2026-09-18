//! Storage-neutral attachment persistence interfaces.

use std::error::Error;
use std::fmt;
use std::future::Future;
use std::pin::Pin;

use codex_utils_image::data_url_from_bytes;
use serde::Deserialize;
use serde::Serialize;

/// Result returned by [`AttachmentStore`] operations.
pub type AttachmentStoreResult<T> = Result<T, AttachmentStoreError>;

/// Future returned by [`AttachmentStore::persist`].
pub type AttachmentStoreFuture<'a> =
    Pin<Box<dyn Future<Output = AttachmentStoreResult<AttachmentRef>> + Send + 'a>>;

/// Stores attachment bytes and returns durable references to them.
///
/// Implementations perform any required I/O before resolving the returned future. A successful
/// reference must remain valid when callers persist it or pass it across subsystem boundaries.
pub trait AttachmentStore: Send + Sync {
    /// Persists `data` and returns a durable reference to it.
    fn persist<'a>(
        &'a self,
        data: &'a [u8],
        metadata: &'a AttachmentMetadata,
    ) -> AttachmentStoreFuture<'a>;
}

/// Metadata used to identify and interpret an attachment.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentMetadata {
    /// Name associated with the attachment.
    pub file_name: String,
    /// IANA media type used to interpret the attachment bytes.
    pub media_type: String,
}

/// Durable reference to bytes stored by an [`AttachmentStore`].
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
pub struct AttachmentRef {
    /// Identifier assigned to an externally stored file.
    pub file_id: Option<String>,
    /// URL referring to the attachment bytes, such as a data URL or
    /// `sediment://<file_id>`.
    pub url: String,
}

impl fmt::Debug for AttachmentRef {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AttachmentRef")
            .field("file_id", &self.file_id)
            .field("url", &"<redacted>")
            .finish()
    }
}

/// Leaves attachments inline instead of storing them.
pub struct InlineAttachmentStore;

impl AttachmentStore for InlineAttachmentStore {
    fn persist<'a>(
        &'a self,
        data: &'a [u8],
        metadata: &'a AttachmentMetadata,
    ) -> AttachmentStoreFuture<'a> {
        Box::pin(async move {
            Ok(AttachmentRef {
                file_id: None,
                url: data_url_from_bytes(&metadata.media_type, data),
            })
        })
    }
}

/// Error returned by an [`AttachmentStore`] implementation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentStoreError {
    message: String,
}

impl AttachmentStoreError {
    /// Creates an error without exposing backend-specific error types.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for AttachmentStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.message.fmt(formatter)
    }
}

impl Error for AttachmentStoreError {}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
