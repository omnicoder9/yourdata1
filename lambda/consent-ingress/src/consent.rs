use std::collections::HashMap;

use async_trait::async_trait;
use aws_sdk_dynamodb::types::AttributeValue;
use aws_sdk_dynamodb::Client as DynamoClient;

use crate::error::IngestionError;
use crate::models::{ConsentRecord, ConsentStatus};

#[async_trait]
pub trait ConsentStore: Send + Sync {
    async fn get_consent(&self, consent_id: &str) -> Result<Option<ConsentRecord>, IngestionError>;
}

pub struct DynamoConsentStore {
    client: DynamoClient,
    table_name: String,
}

impl DynamoConsentStore {
    pub fn new(client: DynamoClient, table_name: String) -> Self {
        Self { client, table_name }
    }
}

#[async_trait]
impl ConsentStore for DynamoConsentStore {
    async fn get_consent(&self, consent_id: &str) -> Result<Option<ConsentRecord>, IngestionError> {
        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("consent_id", AttributeValue::S(consent_id.to_string()))
            .send()
            .await
            .map_err(|e| IngestionError::Internal {
                message: format!("DynamoDB get_item failed: {e}"),
            })?;

        match result.item {
            Some(item) => {
                let record = parse_consent_item(item)?;
                Ok(Some(record))
            }
            None => Ok(None),
        }
    }
}

fn parse_consent_item(
    item: HashMap<String, AttributeValue>,
) -> Result<ConsentRecord, IngestionError> {
    let get_s = |key: &str| -> Result<String, IngestionError> {
        item.get(key)
            .and_then(|v| v.as_s().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| IngestionError::Internal {
                message: format!("Missing or invalid string attribute: {key}"),
            })
    };

    let get_bool = |key: &str| -> Result<bool, IngestionError> {
        item.get(key)
            .and_then(|v| v.as_bool().ok())
            .copied()
            .ok_or_else(|| IngestionError::Internal {
                message: format!("Missing or invalid bool attribute: {key}"),
            })
    };

    let status_str = get_s("status")?;
    let status = match status_str.as_str() {
        "active" => ConsentStatus::Active,
        "revoked" => ConsentStatus::Revoked,
        "expired" => ConsentStatus::Expired,
        other => {
            return Err(IngestionError::Internal {
                message: format!("Unknown consent status: {other}"),
            })
        }
    };

    Ok(ConsentRecord {
        consent_id: get_s("consent_id")?,
        status,
        jurisdiction: get_s("jurisdiction")?,
        policy_version: get_s("policy_version")?,
        analytics_opt_in: get_bool("analytics_opt_in")?,
        marketing_opt_in: get_bool("marketing_opt_in")?,
        personalization_opt_in: get_bool("personalization_opt_in")?,
        data_processing_accepted: get_bool("data_processing_accepted")?,
        created_at: get_s("created_at")?,
        updated_at: item
            .get("updated_at")
            .and_then(|v| v.as_s().ok())
            .map(|s| s.to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_item(status: &str, analytics: bool, marketing: bool, personalization: bool) -> HashMap<String, AttributeValue> {
        let mut item = HashMap::new();
        item.insert("consent_id".to_string(), AttributeValue::S("abc-123".to_string()));
        item.insert("status".to_string(), AttributeValue::S(status.to_string()));
        item.insert("jurisdiction".to_string(), AttributeValue::S("EU".to_string()));
        item.insert("policy_version".to_string(), AttributeValue::S("v1.0".to_string()));
        item.insert("analytics_opt_in".to_string(), AttributeValue::Bool(analytics));
        item.insert("marketing_opt_in".to_string(), AttributeValue::Bool(marketing));
        item.insert("personalization_opt_in".to_string(), AttributeValue::Bool(personalization));
        item.insert("data_processing_accepted".to_string(), AttributeValue::Bool(true));
        item.insert("created_at".to_string(), AttributeValue::S("2025-01-01T00:00:00Z".to_string()));
        item
    }

    #[test]
    fn test_parse_active_consent() {
        let item = make_item("active", true, false, true);
        let record = parse_consent_item(item).unwrap();
        assert_eq!(record.status, ConsentStatus::Active);
        assert!(record.analytics_opt_in);
        assert!(!record.marketing_opt_in);
        assert!(record.personalization_opt_in);
        assert!(record.data_processing_accepted);
    }

    #[test]
    fn test_parse_revoked_consent() {
        let item = make_item("revoked", false, false, false);
        let record = parse_consent_item(item).unwrap();
        assert_eq!(record.status, ConsentStatus::Revoked);
    }

    #[test]
    fn test_parse_expired_consent() {
        let item = make_item("expired", false, false, false);
        let record = parse_consent_item(item).unwrap();
        assert_eq!(record.status, ConsentStatus::Expired);
    }

    #[test]
    fn test_parse_unknown_status_fails() {
        let item = make_item("unknown", false, false, false);
        let err = parse_consent_item(item).unwrap_err();
        assert!(matches!(err, IngestionError::Internal { .. }));
    }

    #[test]
    fn test_parse_missing_field_fails() {
        let mut item = make_item("active", true, true, true);
        item.remove("jurisdiction");
        let err = parse_consent_item(item).unwrap_err();
        assert!(matches!(err, IngestionError::Internal { .. }));
    }
}
