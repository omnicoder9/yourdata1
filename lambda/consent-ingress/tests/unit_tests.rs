use async_trait::async_trait;
use consent_ingress::consent::ConsentStore;
use consent_ingress::crypto::FieldEncryptor;
use consent_ingress::error::{ErrorCode, IngestionError};
use consent_ingress::handler::IngestHandler;
use consent_ingress::models::*;
use consent_ingress::queue::EventQueue;

// ---------------------------------------------------------------------------
// Mock implementations
// ---------------------------------------------------------------------------

struct MockConsentStore {
    record: Option<ConsentRecord>,
}

#[async_trait]
impl ConsentStore for MockConsentStore {
    async fn get_consent(&self, _id: &str) -> Result<Option<ConsentRecord>, IngestionError> {
        Ok(self.record.clone())
    }
}

struct MockEventQueue;

#[async_trait]
impl EventQueue for MockEventQueue {
    async fn enqueue(&self, _msg: &QueueMessage) -> Result<String, IngestionError> {
        Ok("mock-msg-id".to_string())
    }
}

struct MockEncryptor;

#[async_trait]
impl FieldEncryptor for MockEncryptor {
    async fn encrypt_field(&self, plaintext: &str) -> Result<String, IngestionError> {
        Ok(format!("ENC:{plaintext}"))
    }
}

struct FailingConsentStore;

#[async_trait]
impl ConsentStore for FailingConsentStore {
    async fn get_consent(&self, _id: &str) -> Result<Option<ConsentRecord>, IngestionError> {
        Err(IngestionError::Internal {
            message: "DynamoDB timeout".to_string(),
        })
    }
}

struct FailingEncryptor;

#[async_trait]
impl FieldEncryptor for FailingEncryptor {
    async fn encrypt_field(&self, _plaintext: &str) -> Result<String, IngestionError> {
        Err(IngestionError::EncryptionFailed {
            message: "KMS unavailable".to_string(),
        })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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
        payload: serde_json::json!({"url": "/home", "referrer": "https://example.com"}),
        user_id: None,
        jurisdiction: "EU".to_string(),
        policy_version: "v1.0".to_string(),
        purpose: "analytics".to_string(),
        sensitive_fields: None,
    }
}

fn handler_with(
    consent: Option<ConsentRecord>,
) -> IngestHandler<MockConsentStore, MockEventQueue, MockEncryptor> {
    IngestHandler::new(
        MockConsentStore { record: consent },
        MockEventQueue,
        MockEncryptor,
    )
}

// ---------------------------------------------------------------------------
// Happy-path tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn accept_valid_analytics_event() {
    let h = handler_with(Some(active_consent()));
    let resp = h.handle(valid_request()).await.unwrap();
    assert_eq!(resp.status, "accepted");
    assert!(!resp.queue_message_id.is_empty());
}

#[tokio::test]
async fn accept_valid_personalization_event() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.purpose = "personalization".to_string();
    let resp = h.handle(req).await.unwrap();
    assert_eq!(resp.status, "accepted");
}

#[tokio::test]
async fn accept_valid_general_event() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.purpose = "general".to_string();
    let resp = h.handle(req).await.unwrap();
    assert_eq!(resp.status, "accepted");
}

#[tokio::test]
async fn accept_event_with_user_id() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.user_id = Some("b2c3d4e5-f6a7-8901-bcde-f12345678901".to_string());
    let resp = h.handle(req).await.unwrap();
    assert_eq!(resp.status, "accepted");
}

#[tokio::test]
async fn accept_event_with_sensitive_fields() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.payload = serde_json::json!({"email": "a@b.com", "url": "/home"});
    req.sensitive_fields = Some(vec!["email".to_string()]);
    let resp = h.handle(req).await.unwrap();
    assert_eq!(resp.status, "accepted");
}

// ---------------------------------------------------------------------------
// Consent-gate rejection tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reject_when_consent_not_found() {
    let h = handler_with(None);
    let err = h.handle(valid_request()).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::ConsentNotFound);
    assert_eq!(err.status_code(), 404);
}

#[tokio::test]
async fn reject_when_consent_revoked() {
    let mut c = active_consent();
    c.status = ConsentStatus::Revoked;
    let h = handler_with(Some(c));
    let err = h.handle(valid_request()).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::ConsentRevoked);
    assert_eq!(err.status_code(), 403);
}

#[tokio::test]
async fn reject_when_consent_expired() {
    let mut c = active_consent();
    c.status = ConsentStatus::Expired;
    let h = handler_with(Some(c));
    let err = h.handle(valid_request()).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::ConsentExpired);
    assert_eq!(err.status_code(), 403);
}

#[tokio::test]
async fn reject_when_purpose_not_consented() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.purpose = "marketing".to_string(); // marketing_opt_in is false
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::PurposeNotConsented);
    assert_eq!(err.status_code(), 403);

    let api_resp = err.to_api_response();
    let details = api_resp.details.unwrap();
    assert_eq!(details["rejected_purpose"], "marketing");
}

#[tokio::test]
async fn reject_when_policy_version_mismatch() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.policy_version = "v999.0".to_string();
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::PolicyVersionMismatch);
    assert_eq!(err.status_code(), 409);

    let api_resp = err.to_api_response();
    let details = api_resp.details.unwrap();
    assert_eq!(details["expected_version"], "v1.0");
    assert_eq!(details["actual_version"], "v999.0");
}

// ---------------------------------------------------------------------------
// Schema validation rejection tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reject_empty_consent_id() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.consent_id = String::new();
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::SchemaValidationFailed);
    assert_eq!(err.status_code(), 400);
}

#[tokio::test]
async fn reject_non_uuid_consent_id() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.consent_id = "not-a-uuid".to_string();
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::SchemaValidationFailed);
}

#[tokio::test]
async fn reject_empty_event_type() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.event_type = String::new();
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::SchemaValidationFailed);
}

#[tokio::test]
async fn reject_non_object_payload() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.payload = serde_json::json!([1, 2, 3]);
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::SchemaValidationFailed);
}

#[tokio::test]
async fn reject_invalid_jurisdiction() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.jurisdiction = "INVALID".to_string();
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::SchemaValidationFailed);
}

#[tokio::test]
async fn reject_invalid_purpose() {
    let h = handler_with(Some(active_consent()));
    let mut req = valid_request();
    req.purpose = "unknown_purpose".to_string();
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::SchemaValidationFailed);
}

// ---------------------------------------------------------------------------
// Infrastructure failure tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn internal_error_on_consent_store_failure() {
    let h = IngestHandler::new(FailingConsentStore, MockEventQueue, MockEncryptor);
    let err = h.handle(valid_request()).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::InternalError);
    assert_eq!(err.status_code(), 500);
}

#[tokio::test]
async fn encryption_failure_returns_500() {
    let h = IngestHandler::new(
        MockConsentStore {
            record: Some(active_consent()),
        },
        MockEventQueue,
        FailingEncryptor,
    );
    let mut req = valid_request();
    req.payload = serde_json::json!({"email": "a@b.com"});
    req.sensitive_fields = Some(vec!["email".to_string()]);
    let err = h.handle(req).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::EncryptionFailed);
    assert_eq!(err.status_code(), 500);
}

#[tokio::test]
async fn queue_failure_returns_502() {
    struct FailingQueue;

    #[async_trait]
    impl EventQueue for FailingQueue {
        async fn enqueue(&self, _msg: &QueueMessage) -> Result<String, IngestionError> {
            Err(IngestionError::QueueFailed {
                message: "SQS down".to_string(),
            })
        }
    }

    let h = IngestHandler::new(
        MockConsentStore {
            record: Some(active_consent()),
        },
        FailingQueue,
        MockEncryptor,
    );
    let err = h.handle(valid_request()).await.unwrap_err();
    assert_eq!(err.error_code(), ErrorCode::QueueFailed);
    assert_eq!(err.status_code(), 502);
}

// ---------------------------------------------------------------------------
// Error response structure tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn error_response_is_json_serializable() {
    let err = IngestionError::ConsentNotFound {
        consent_id: "abc".to_string(),
    };
    let api_resp = err.to_api_response();
    let json = serde_json::to_string(&api_resp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["error_code"], "CONSENT_NOT_FOUND");
    assert!(parsed["message"].as_str().unwrap().contains("abc"));
}

#[tokio::test]
async fn purpose_not_consented_includes_details() {
    let err = IngestionError::PurposeNotConsented {
        consent_id: "abc".to_string(),
        purpose: "marketing".to_string(),
    };
    let api_resp = err.to_api_response();
    let json = serde_json::to_string(&api_resp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["details"]["rejected_purpose"], "marketing");
    assert_eq!(parsed["details"]["consent_id"], "abc");
}

#[tokio::test]
async fn policy_mismatch_includes_versions() {
    let err = IngestionError::PolicyVersionMismatch {
        expected: "v1.0".to_string(),
        actual: "v2.0".to_string(),
    };
    let api_resp = err.to_api_response();
    let json = serde_json::to_string(&api_resp).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["details"]["expected_version"], "v1.0");
    assert_eq!(parsed["details"]["actual_version"], "v2.0");
}

// ---------------------------------------------------------------------------
// Idempotency key tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn idempotency_key_stable_across_calls() {
    use consent_ingress::queue::generate_idempotency_key;

    let payload = serde_json::json!({"url": "/home"});
    let k1 = generate_idempotency_key("abc", "page_view", &payload);
    let k2 = generate_idempotency_key("abc", "page_view", &payload);
    assert_eq!(k1, k2);
}

// ---------------------------------------------------------------------------
// ConsentRecord purpose mapping tests
// ---------------------------------------------------------------------------

#[test]
fn consent_record_purpose_mapping() {
    let consent = active_consent();
    assert!(consent.is_purpose_consented("analytics"));
    assert!(!consent.is_purpose_consented("marketing"));
    assert!(consent.is_purpose_consented("personalization"));
    assert!(consent.is_purpose_consented("general")); // falls through to data_processing_accepted
    assert!(consent.is_purpose_consented("anything_else")); // also data_processing_accepted
}
