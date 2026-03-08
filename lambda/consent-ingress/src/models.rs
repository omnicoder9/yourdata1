use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize)]
pub struct IngestEventRequest {
    pub consent_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    #[serde(default)]
    pub user_id: Option<String>,
    pub jurisdiction: String,
    pub policy_version: String,
    pub purpose: String,
    #[serde(default)]
    pub sensitive_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConsentStatus {
    Active,
    Revoked,
    Expired,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsentRecord {
    pub consent_id: String,
    pub status: ConsentStatus,
    pub jurisdiction: String,
    pub policy_version: String,
    pub analytics_opt_in: bool,
    pub marketing_opt_in: bool,
    pub personalization_opt_in: bool,
    pub data_processing_accepted: bool,
    pub created_at: String,
    #[serde(default)]
    pub updated_at: Option<String>,
}

impl ConsentRecord {
    pub fn is_purpose_consented(&self, purpose: &str) -> bool {
        match purpose {
            "analytics" => self.analytics_opt_in,
            "marketing" => self.marketing_opt_in,
            "personalization" => self.personalization_opt_in,
            _ => self.data_processing_accepted,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueMessage {
    pub event_id: Uuid,
    pub consent_id: String,
    pub correlation_id: Uuid,
    pub idempotency_key: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    pub jurisdiction: String,
    pub policy_version: String,
    pub purpose: String,
    pub ingested_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_fields: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngestEventResponse {
    pub event_id: Uuid,
    pub correlation_id: Uuid,
    pub status: &'static str,
    pub queue_message_id: String,
}
