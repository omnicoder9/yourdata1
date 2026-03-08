use async_trait::async_trait;
use aws_sdk_sqs::types::MessageAttributeValue;
use aws_sdk_sqs::Client as SqsClient;
use sha2::{Digest, Sha256};

use crate::error::IngestionError;
use crate::models::QueueMessage;

#[async_trait]
pub trait EventQueue: Send + Sync {
    async fn enqueue(&self, message: &QueueMessage) -> Result<String, IngestionError>;
}

pub struct SqsEventQueue {
    client: SqsClient,
    queue_url: String,
}

impl SqsEventQueue {
    pub fn new(client: SqsClient, queue_url: String) -> Self {
        Self { client, queue_url }
    }
}

#[async_trait]
impl EventQueue for SqsEventQueue {
    async fn enqueue(&self, message: &QueueMessage) -> Result<String, IngestionError> {
        let body =
            serde_json::to_string(message).map_err(|e| IngestionError::QueueFailed {
                message: format!("Failed to serialize queue message: {e}"),
            })?;

        let result = self
            .client
            .send_message()
            .queue_url(&self.queue_url)
            .message_body(&body)
            .message_attributes(
                "correlation_id",
                MessageAttributeValue::builder()
                    .data_type("String")
                    .string_value(message.correlation_id.to_string())
                    .build()
                    .map_err(|e| IngestionError::QueueFailed {
                        message: format!("Failed to build correlation_id attribute: {e}"),
                    })?,
            )
            .message_attributes(
                "idempotency_key",
                MessageAttributeValue::builder()
                    .data_type("String")
                    .string_value(&message.idempotency_key)
                    .build()
                    .map_err(|e| IngestionError::QueueFailed {
                        message: format!("Failed to build idempotency_key attribute: {e}"),
                    })?,
            )
            .message_attributes(
                "event_type",
                MessageAttributeValue::builder()
                    .data_type("String")
                    .string_value(&message.event_type)
                    .build()
                    .map_err(|e| IngestionError::QueueFailed {
                        message: format!("Failed to build event_type attribute: {e}"),
                    })?,
            )
            .message_attributes(
                "jurisdiction",
                MessageAttributeValue::builder()
                    .data_type("String")
                    .string_value(&message.jurisdiction)
                    .build()
                    .map_err(|e| IngestionError::QueueFailed {
                        message: format!("Failed to build jurisdiction attribute: {e}"),
                    })?,
            )
            .send()
            .await
            .map_err(|e| IngestionError::QueueFailed {
                message: format!("SQS send_message failed: {e}"),
            })?;

        result
            .message_id
            .ok_or_else(|| IngestionError::QueueFailed {
                message: "SQS returned no message_id".to_string(),
            })
    }
}

pub fn generate_idempotency_key(
    consent_id: &str,
    event_type: &str,
    payload: &serde_json::Value,
) -> String {
    let canonical_payload = serde_json::to_string(payload).unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(consent_id.as_bytes());
    hasher.update(b"|");
    hasher.update(event_type.as_bytes());
    hasher.update(b"|");
    hasher.update(canonical_payload.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_idempotency_key_deterministic() {
        let key1 = generate_idempotency_key(
            "abc-123",
            "page_view",
            &serde_json::json!({"url": "/home"}),
        );
        let key2 = generate_idempotency_key(
            "abc-123",
            "page_view",
            &serde_json::json!({"url": "/home"}),
        );
        assert_eq!(key1, key2);
    }

    #[test]
    fn test_idempotency_key_differs_by_consent() {
        let key1 = generate_idempotency_key(
            "abc-123",
            "page_view",
            &serde_json::json!({"url": "/home"}),
        );
        let key2 = generate_idempotency_key(
            "def-456",
            "page_view",
            &serde_json::json!({"url": "/home"}),
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_idempotency_key_differs_by_event_type() {
        let key1 = generate_idempotency_key(
            "abc-123",
            "page_view",
            &serde_json::json!({"url": "/home"}),
        );
        let key2 = generate_idempotency_key(
            "abc-123",
            "click",
            &serde_json::json!({"url": "/home"}),
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_idempotency_key_differs_by_payload() {
        let key1 = generate_idempotency_key(
            "abc-123",
            "page_view",
            &serde_json::json!({"url": "/home"}),
        );
        let key2 = generate_idempotency_key(
            "abc-123",
            "page_view",
            &serde_json::json!({"url": "/about"}),
        );
        assert_ne!(key1, key2);
    }

    #[test]
    fn test_idempotency_key_is_hex_sha256() {
        let key = generate_idempotency_key("a", "b", &serde_json::json!({}));
        assert_eq!(key.len(), 64);
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
