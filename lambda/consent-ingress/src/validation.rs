use uuid::Uuid;

use crate::error::IngestionError;
use crate::models::IngestEventRequest;

const VALID_JURISDICTIONS: &[&str] = &["EU", "UK", "US-CA", "US-OTHER", "OTHER"];
const VALID_PURPOSES: &[&str] = &["analytics", "marketing", "personalization", "general"];

pub fn validate_request(req: &IngestEventRequest) -> Result<(), IngestionError> {
    if req.consent_id.is_empty() {
        return Err(IngestionError::SchemaValidation {
            message: "consent_id is required".to_string(),
        });
    }

    Uuid::parse_str(&req.consent_id).map_err(|_| IngestionError::SchemaValidation {
        message: format!("consent_id '{}' is not a valid UUID", req.consent_id),
    })?;

    if req.event_type.is_empty() {
        return Err(IngestionError::SchemaValidation {
            message: "event_type is required".to_string(),
        });
    }

    if req.event_type.len() > 256 {
        return Err(IngestionError::SchemaValidation {
            message: "event_type must not exceed 256 characters".to_string(),
        });
    }

    if !req.payload.is_object() {
        return Err(IngestionError::SchemaValidation {
            message: "payload must be a JSON object".to_string(),
        });
    }

    if let Some(ref user_id) = req.user_id {
        if !user_id.is_empty() {
            Uuid::parse_str(user_id).map_err(|_| IngestionError::SchemaValidation {
                message: format!("user_id '{user_id}' is not a valid UUID"),
            })?;
        }
    }

    if req.jurisdiction.is_empty() {
        return Err(IngestionError::SchemaValidation {
            message: "jurisdiction is required".to_string(),
        });
    }

    if !VALID_JURISDICTIONS.contains(&req.jurisdiction.as_str()) {
        return Err(IngestionError::SchemaValidation {
            message: format!(
                "jurisdiction '{}' is not valid; expected one of: {}",
                req.jurisdiction,
                VALID_JURISDICTIONS.join(", ")
            ),
        });
    }

    if req.policy_version.is_empty() {
        return Err(IngestionError::SchemaValidation {
            message: "policy_version is required".to_string(),
        });
    }

    if req.purpose.is_empty() {
        return Err(IngestionError::SchemaValidation {
            message: "purpose is required".to_string(),
        });
    }

    if !VALID_PURPOSES.contains(&req.purpose.as_str()) {
        return Err(IngestionError::SchemaValidation {
            message: format!(
                "purpose '{}' is not valid; expected one of: {}",
                req.purpose,
                VALID_PURPOSES.join(", ")
            ),
        });
    }

    if let Some(ref fields) = req.sensitive_fields {
        let payload_obj = req.payload.as_object().expect("validated above");
        for field in fields {
            if !payload_obj.contains_key(field) {
                return Err(IngestionError::SchemaValidation {
                    message: format!(
                        "sensitive_field '{field}' not found in payload"
                    ),
                });
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_valid_request_passes() {
        assert!(validate_request(&valid_request()).is_ok());
    }

    #[test]
    fn test_empty_consent_id_rejected() {
        let mut req = valid_request();
        req.consent_id = String::new();
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_invalid_consent_id_uuid_rejected() {
        let mut req = valid_request();
        req.consent_id = "not-a-uuid".to_string();
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_empty_event_type_rejected() {
        let mut req = valid_request();
        req.event_type = String::new();
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_event_type_too_long_rejected() {
        let mut req = valid_request();
        req.event_type = "a".repeat(257);
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_payload_not_object_rejected() {
        let mut req = valid_request();
        req.payload = serde_json::json!("a string");
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_invalid_user_id_uuid_rejected() {
        let mut req = valid_request();
        req.user_id = Some("bad-uuid".to_string());
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_invalid_jurisdiction_rejected() {
        let mut req = valid_request();
        req.jurisdiction = "MARS".to_string();
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_empty_policy_version_rejected() {
        let mut req = valid_request();
        req.policy_version = String::new();
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_invalid_purpose_rejected() {
        let mut req = valid_request();
        req.purpose = "surveillance".to_string();
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_sensitive_field_missing_from_payload_rejected() {
        let mut req = valid_request();
        req.sensitive_fields = Some(vec!["email".to_string()]);
        let err = validate_request(&req).unwrap_err();
        assert!(matches!(err, IngestionError::SchemaValidation { .. }));
    }

    #[test]
    fn test_sensitive_field_present_in_payload_passes() {
        let mut req = valid_request();
        req.payload = serde_json::json!({"email": "user@example.com"});
        req.sensitive_fields = Some(vec!["email".to_string()]);
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn test_all_jurisdictions_accepted() {
        for j in &["EU", "UK", "US-CA", "US-OTHER", "OTHER"] {
            let mut req = valid_request();
            req.jurisdiction = j.to_string();
            assert!(validate_request(&req).is_ok(), "jurisdiction {j} should be valid");
        }
    }

    #[test]
    fn test_all_purposes_accepted() {
        for p in &["analytics", "marketing", "personalization", "general"] {
            let mut req = valid_request();
            req.purpose = p.to_string();
            assert!(validate_request(&req).is_ok(), "purpose {p} should be valid");
        }
    }
}
