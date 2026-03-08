use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ErrorCode {
    #[serde(rename = "SCHEMA_VALIDATION_FAILED")]
    SchemaValidationFailed,
    #[serde(rename = "CONSENT_NOT_FOUND")]
    ConsentNotFound,
    #[serde(rename = "CONSENT_REVOKED")]
    ConsentRevoked,
    #[serde(rename = "CONSENT_EXPIRED")]
    ConsentExpired,
    #[serde(rename = "PURPOSE_NOT_CONSENTED")]
    PurposeNotConsented,
    #[serde(rename = "POLICY_VERSION_MISMATCH")]
    PolicyVersionMismatch,
    #[serde(rename = "ENCRYPTION_FAILED")]
    EncryptionFailed,
    #[serde(rename = "QUEUE_FAILED")]
    QueueFailed,
    #[serde(rename = "INTERNAL_ERROR")]
    InternalError,
}

#[derive(Debug, Error)]
pub enum IngestionError {
    #[error("Schema validation failed: {message}")]
    SchemaValidation { message: String },

    #[error("Consent record not found for id: {consent_id}")]
    ConsentNotFound { consent_id: String },

    #[error("Consent has been revoked for id: {consent_id}")]
    ConsentRevoked { consent_id: String },

    #[error("Consent has expired for id: {consent_id}")]
    ConsentExpired { consent_id: String },

    #[error("Purpose '{purpose}' not consented for id: {consent_id}")]
    PurposeNotConsented {
        consent_id: String,
        purpose: String,
    },

    #[error("Policy version mismatch: expected '{expected}', got '{actual}'")]
    PolicyVersionMismatch { expected: String, actual: String },

    #[error("Field encryption failed: {message}")]
    EncryptionFailed { message: String },

    #[error("Failed to enqueue event: {message}")]
    QueueFailed { message: String },

    #[error("Internal error: {message}")]
    Internal { message: String },
}

#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    pub error_code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl IngestionError {
    pub fn error_code(&self) -> ErrorCode {
        match self {
            Self::SchemaValidation { .. } => ErrorCode::SchemaValidationFailed,
            Self::ConsentNotFound { .. } => ErrorCode::ConsentNotFound,
            Self::ConsentRevoked { .. } => ErrorCode::ConsentRevoked,
            Self::ConsentExpired { .. } => ErrorCode::ConsentExpired,
            Self::PurposeNotConsented { .. } => ErrorCode::PurposeNotConsented,
            Self::PolicyVersionMismatch { .. } => ErrorCode::PolicyVersionMismatch,
            Self::EncryptionFailed { .. } => ErrorCode::EncryptionFailed,
            Self::QueueFailed { .. } => ErrorCode::QueueFailed,
            Self::Internal { .. } => ErrorCode::InternalError,
        }
    }

    pub fn status_code(&self) -> u16 {
        match self {
            Self::SchemaValidation { .. } => 400,
            Self::ConsentNotFound { .. } => 404,
            Self::ConsentRevoked { .. } => 403,
            Self::ConsentExpired { .. } => 403,
            Self::PurposeNotConsented { .. } => 403,
            Self::PolicyVersionMismatch { .. } => 409,
            Self::EncryptionFailed { .. } => 500,
            Self::QueueFailed { .. } => 502,
            Self::Internal { .. } => 500,
        }
    }

    pub fn to_api_response(&self) -> ApiErrorResponse {
        let details = match self {
            Self::PurposeNotConsented {
                consent_id,
                purpose,
            } => Some(serde_json::json!({
                "consent_id": consent_id,
                "rejected_purpose": purpose,
            })),
            Self::PolicyVersionMismatch { expected, actual } => Some(serde_json::json!({
                "expected_version": expected,
                "actual_version": actual,
            })),
            _ => None,
        };

        ApiErrorResponse {
            error_code: self.error_code(),
            message: self.to_string(),
            details,
        }
    }
}
