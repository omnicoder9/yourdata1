use chrono::Utc;
use uuid::Uuid;

use crate::consent::ConsentStore;
use crate::crypto::{encrypt_sensitive_fields, FieldEncryptor};
use crate::error::IngestionError;
use crate::models::{
    ConsentStatus, IngestEventRequest, IngestEventResponse, QueueMessage,
};
use crate::queue::{generate_idempotency_key, EventQueue};
use crate::validation::validate_request;

pub struct IngestHandler<C, Q, E> {
    pub consent_store: C,
    pub event_queue: Q,
    pub field_encryptor: E,
}

impl<C: ConsentStore, Q: EventQueue, E: FieldEncryptor> IngestHandler<C, Q, E> {
    pub fn new(consent_store: C, event_queue: Q, field_encryptor: E) -> Self {
        Self {
            consent_store,
            event_queue,
            field_encryptor,
        }
    }

    pub async fn handle(
        &self,
        request: IngestEventRequest,
    ) -> Result<IngestEventResponse, IngestionError> {
        validate_request(&request)?;

        let consent = self
            .consent_store
            .get_consent(&request.consent_id)
            .await?
            .ok_or_else(|| IngestionError::ConsentNotFound {
                consent_id: request.consent_id.clone(),
            })?;

        match consent.status {
            ConsentStatus::Active => {}
            ConsentStatus::Revoked => {
                return Err(IngestionError::ConsentRevoked {
                    consent_id: request.consent_id.clone(),
                });
            }
            ConsentStatus::Expired => {
                return Err(IngestionError::ConsentExpired {
                    consent_id: request.consent_id.clone(),
                });
            }
        }

        if consent.policy_version != request.policy_version {
            return Err(IngestionError::PolicyVersionMismatch {
                expected: consent.policy_version.clone(),
                actual: request.policy_version.clone(),
            });
        }

        if !consent.is_purpose_consented(&request.purpose) {
            return Err(IngestionError::PurposeNotConsented {
                consent_id: request.consent_id.clone(),
                purpose: request.purpose.clone(),
            });
        }

        let mut payload = request.payload.clone();
        let encrypted_fields = if let Some(ref fields) = request.sensitive_fields {
            if !fields.is_empty() {
                Some(
                    encrypt_sensitive_fields(&self.field_encryptor, &mut payload, fields)
                        .await?,
                )
            } else {
                None
            }
        } else {
            None
        };

        let event_id = Uuid::new_v4();
        let correlation_id = Uuid::new_v4();
        let idempotency_key =
            generate_idempotency_key(&request.consent_id, &request.event_type, &request.payload);

        let queue_message = QueueMessage {
            event_id,
            consent_id: request.consent_id,
            correlation_id,
            idempotency_key,
            event_type: request.event_type,
            payload,
            user_id: request.user_id,
            jurisdiction: request.jurisdiction,
            policy_version: request.policy_version,
            purpose: request.purpose,
            ingested_at: Utc::now().to_rfc3339(),
            encrypted_fields,
        };

        let message_id = self.event_queue.enqueue(&queue_message).await?;

        Ok(IngestEventResponse {
            event_id,
            correlation_id,
            status: "accepted",
            queue_message_id: message_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::ConsentStore;
    use crate::crypto::FieldEncryptor;
    use crate::models::{ConsentRecord, ConsentStatus};
    use crate::queue::EventQueue;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct MockConsentStore {
        record: Option<ConsentRecord>,
    }

    #[async_trait]
    impl ConsentStore for MockConsentStore {
        async fn get_consent(
            &self,
            _consent_id: &str,
        ) -> Result<Option<ConsentRecord>, IngestionError> {
            Ok(self.record.clone())
        }
    }

    struct MockEventQueue {
        enqueue_count: Arc<AtomicUsize>,
    }

    impl MockEventQueue {
        fn new() -> Self {
            Self {
                enqueue_count: Arc::new(AtomicUsize::new(0)),
            }
        }
    }

    #[async_trait]
    impl EventQueue for MockEventQueue {
        async fn enqueue(&self, _message: &QueueMessage) -> Result<String, IngestionError> {
            self.enqueue_count.fetch_add(1, Ordering::SeqCst);
            Ok("mock-message-id-001".to_string())
        }
    }

    struct MockEncryptor;

    #[async_trait]
    impl FieldEncryptor for MockEncryptor {
        async fn encrypt_field(&self, plaintext: &str) -> Result<String, IngestionError> {
            Ok(format!("ENC:{plaintext}"))
        }
    }

    fn active_consent() -> ConsentRecord {
        ConsentRecord {
            consent_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
            status: ConsentStatus::Active,
            jurisdiction: "EU".to_string(),
            policy_version: "v1.0".to_string(),
            analytics_opt_in: true,
            marketing_opt_in: false,
            personalization_opt_in: true,
            data_processing_accepted: true,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: None,
        }
    }

    fn valid_request() -> IngestEventRequest {
        IngestEventRequest {
            consent_id: "a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string(),
            event_type: "page_view".to_string(),
            payload: serde_json::json!({"url": "/home"}),
            user_id: None,
            jurisdiction: "EU".to_string(),
            policy_version: "v1.0".to_string(),
            purpose: "analytics".to_string(),
            sensitive_fields: None,
        }
    }

    #[tokio::test]
    async fn test_valid_event_accepted() {
        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let result = handler.handle(valid_request()).await.unwrap();
        assert_eq!(result.status, "accepted");
        assert_eq!(result.queue_message_id, "mock-message-id-001");
    }

    #[tokio::test]
    async fn test_consent_not_found_returns_error() {
        let handler = IngestHandler::new(
            MockConsentStore { record: None },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let err = handler.handle(valid_request()).await.unwrap_err();
        assert!(matches!(err, IngestionError::ConsentNotFound { .. }));
        assert_eq!(err.status_code(), 404);
    }

    #[tokio::test]
    async fn test_revoked_consent_returns_error() {
        let mut consent = active_consent();
        consent.status = ConsentStatus::Revoked;

        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(consent),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let err = handler.handle(valid_request()).await.unwrap_err();
        assert!(matches!(err, IngestionError::ConsentRevoked { .. }));
        assert_eq!(err.status_code(), 403);
    }

    #[tokio::test]
    async fn test_expired_consent_returns_error() {
        let mut consent = active_consent();
        consent.status = ConsentStatus::Expired;

        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(consent),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let err = handler.handle(valid_request()).await.unwrap_err();
        assert!(matches!(err, IngestionError::ConsentExpired { .. }));
        assert_eq!(err.status_code(), 403);
    }

    #[tokio::test]
    async fn test_policy_version_mismatch_returns_error() {
        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let mut req = valid_request();
        req.policy_version = "v2.0".to_string();

        let err = handler.handle(req).await.unwrap_err();
        assert!(matches!(err, IngestionError::PolicyVersionMismatch { .. }));
        assert_eq!(err.status_code(), 409);
    }

    #[tokio::test]
    async fn test_purpose_not_consented_returns_error() {
        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let mut req = valid_request();
        req.purpose = "marketing".to_string();

        let err = handler.handle(req).await.unwrap_err();
        assert!(matches!(err, IngestionError::PurposeNotConsented { .. }));
        assert_eq!(err.status_code(), 403);
    }

    #[tokio::test]
    async fn test_schema_validation_error_returns_400() {
        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let mut req = valid_request();
        req.consent_id = String::new();

        let err = handler.handle(req).await.unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
        assert_eq!(err.status_code(), 400);
    }

    #[tokio::test]
    async fn test_sensitive_fields_encrypted() {
        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let mut req = valid_request();
        req.payload = serde_json::json!({"email": "user@test.com", "url": "/home"});
        req.sensitive_fields = Some(vec!["email".to_string()]);

        let result = handler.handle(req).await.unwrap();
        assert_eq!(result.status, "accepted");
    }

    #[tokio::test]
    async fn test_queue_failure_returns_502() {
        struct FailingQueue;

        #[async_trait]
        impl EventQueue for FailingQueue {
            async fn enqueue(&self, _message: &QueueMessage) -> Result<String, IngestionError> {
                Err(IngestionError::QueueFailed {
                    message: "SQS unavailable".to_string(),
                })
            }
        }

        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            FailingQueue,
            MockEncryptor,
        );

        let err = handler.handle(valid_request()).await.unwrap_err();
        assert!(matches!(err, IngestionError::QueueFailed { .. }));
        assert_eq!(err.status_code(), 502);
    }

    #[tokio::test]
    async fn test_general_purpose_uses_data_processing_accepted() {
        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let mut req = valid_request();
        req.purpose = "general".to_string();

        let result = handler.handle(req).await.unwrap();
        assert_eq!(result.status, "accepted");
    }

    #[tokio::test]
    async fn test_personalization_purpose_when_consented() {
        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            MockEventQueue::new(),
            MockEncryptor,
        );

        let mut req = valid_request();
        req.purpose = "personalization".to_string();

        let result = handler.handle(req).await.unwrap();
        assert_eq!(result.status, "accepted");
    }

    #[tokio::test]
    async fn test_enqueue_called_exactly_once() {
        let queue = MockEventQueue::new();
        let count = queue.enqueue_count.clone();

        let handler = IngestHandler::new(
            MockConsentStore {
                record: Some(active_consent()),
            },
            queue,
            MockEncryptor,
        );

        handler.handle(valid_request()).await.unwrap();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
