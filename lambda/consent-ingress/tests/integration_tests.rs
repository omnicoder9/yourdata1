// Integration tests for consent-gated ingress.
//
// These tests exercise the real AWS SDK clients against either:
//   - LocalStack (set LOCALSTACK_ENDPOINT=http://localhost:4566)
//   - Real AWS (provide standard AWS credentials + region)
//
// Run with:
//   INTEGRATION=1 cargo test --test integration_tests
//
// Required env vars:
//   INTEGRATION=1                   (gate so unit-test runs skip these)
//   AWS_REGION or AWS_DEFAULT_REGION
//   CONSENT_TABLE_NAME              (DynamoDB table, pre-created)
//   EVENT_QUEUE_URL                 (SQS queue URL, pre-created)
//   KMS_KEY_ID                      (optional; skip encryption tests if unset)
//   LOCALSTACK_ENDPOINT             (optional; e.g. http://localhost:4566)

use std::collections::HashMap;

use aws_sdk_dynamodb::types::AttributeValue;
use uuid::Uuid;

use consent_ingress::consent::DynamoConsentStore;
use consent_ingress::crypto::{KmsFieldEncryptor, NoOpEncryptor, FieldEncryptor};
use consent_ingress::handler::IngestHandler;
use consent_ingress::models::IngestEventRequest;
use consent_ingress::queue::SqsEventQueue;

fn should_run() -> bool {
    std::env::var("INTEGRATION").unwrap_or_default() == "1"
}

fn localstack_endpoint() -> Option<String> {
    std::env::var("LOCALSTACK_ENDPOINT").ok()
}

async fn make_aws_config() -> aws_config::SdkConfig {
    let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest());

    if let Some(endpoint) = localstack_endpoint() {
        loader = loader.endpoint_url(&endpoint);
    }

    loader.load().await
}

fn consent_table() -> String {
    std::env::var("CONSENT_TABLE_NAME").unwrap_or_else(|_| "yourdata-consents".to_string())
}

fn queue_url() -> String {
    std::env::var("EVENT_QUEUE_URL").expect("EVENT_QUEUE_URL required for integration tests")
}

fn kms_key_id() -> Option<String> {
    std::env::var("KMS_KEY_ID").ok()
}

async fn seed_consent(
    client: &aws_sdk_dynamodb::Client,
    table: &str,
    consent_id: &str,
    status: &str,
    policy_version: &str,
    analytics: bool,
    marketing: bool,
    personalization: bool,
) {
    let mut item = HashMap::new();
    item.insert(
        "consent_id".to_string(),
        AttributeValue::S(consent_id.to_string()),
    );
    item.insert("status".to_string(), AttributeValue::S(status.to_string()));
    item.insert(
        "jurisdiction".to_string(),
        AttributeValue::S("EU".to_string()),
    );
    item.insert(
        "policy_version".to_string(),
        AttributeValue::S(policy_version.to_string()),
    );
    item.insert(
        "analytics_opt_in".to_string(),
        AttributeValue::Bool(analytics),
    );
    item.insert(
        "marketing_opt_in".to_string(),
        AttributeValue::Bool(marketing),
    );
    item.insert(
        "personalization_opt_in".to_string(),
        AttributeValue::Bool(personalization),
    );
    item.insert(
        "data_processing_accepted".to_string(),
        AttributeValue::Bool(true),
    );
    item.insert(
        "created_at".to_string(),
        AttributeValue::S(chrono::Utc::now().to_rfc3339()),
    );

    client
        .put_item()
        .table_name(table)
        .set_item(Some(item))
        .send()
        .await
        .expect("Failed to seed consent record");
}

async fn cleanup_consent(client: &aws_sdk_dynamodb::Client, table: &str, consent_id: &str) {
    let _ = client
        .delete_item()
        .table_name(table)
        .key("consent_id", AttributeValue::S(consent_id.to_string()))
        .send()
        .await;
}

async fn purge_queue(client: &aws_sdk_sqs::Client, url: &str) {
    let _ = client.purge_queue().queue_url(url).send().await;
}

async fn receive_message(
    client: &aws_sdk_sqs::Client,
    url: &str,
) -> Option<String> {
    let resp = client
        .receive_message()
        .queue_url(url)
        .max_number_of_messages(1)
        .wait_time_seconds(5)
        .message_attribute_names("All")
        .send()
        .await
        .ok()?;

    resp.messages
        .and_then(|msgs| msgs.into_iter().next())
        .and_then(|msg| msg.body)
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn integration_happy_path_event_flows_through() {
    if !should_run() {
        eprintln!("Skipping integration test (set INTEGRATION=1 to run)");
        return;
    }

    let config = make_aws_config().await;
    let dynamo = aws_sdk_dynamodb::Client::new(&config);
    let sqs = aws_sdk_sqs::Client::new(&config);

    let table = consent_table();
    let url = queue_url();
    let consent_id = Uuid::new_v4().to_string();

    // Setup
    seed_consent(&dynamo, &table, &consent_id, "active", "v1.0", true, true, true).await;
    purge_queue(&sqs, &url).await;

    // Build handler
    let consent_store = DynamoConsentStore::new(dynamo.clone(), table.clone());
    let event_queue = SqsEventQueue::new(sqs.clone(), url.clone());
    let encryptor: Box<dyn FieldEncryptor> = if let Some(key_id) = kms_key_id() {
        let kms = aws_sdk_kms::Client::new(&config);
        Box::new(KmsFieldEncryptor::new(kms, key_id))
    } else {
        Box::new(NoOpEncryptor)
    };

    let handler = IngestHandler::new(consent_store, event_queue, encryptor);

    let request = IngestEventRequest {
        consent_id: consent_id.clone(),
        event_type: "page_view".to_string(),
        payload: serde_json::json!({"url": "/integration-test"}),
        user_id: None,
        jurisdiction: "EU".to_string(),
        policy_version: "v1.0".to_string(),
        purpose: "analytics".to_string(),
        sensitive_fields: None,
    };

    let response = handler.handle(request).await.expect("should succeed");
    assert_eq!(response.status, "accepted");

    // Verify message landed in SQS
    let body = receive_message(&sqs, &url).await.expect("expected SQS message");
    let msg: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(msg["consent_id"], consent_id);
    assert_eq!(msg["event_type"], "page_view");
    assert_eq!(msg["jurisdiction"], "EU");

    // Cleanup
    cleanup_consent(&dynamo, &table, &consent_id).await;
}

#[tokio::test]
async fn integration_revoked_consent_rejected() {
    if !should_run() {
        eprintln!("Skipping integration test (set INTEGRATION=1 to run)");
        return;
    }

    let config = make_aws_config().await;
    let dynamo = aws_sdk_dynamodb::Client::new(&config);
    let sqs = aws_sdk_sqs::Client::new(&config);

    let table = consent_table();
    let url = queue_url();
    let consent_id = Uuid::new_v4().to_string();

    seed_consent(&dynamo, &table, &consent_id, "revoked", "v1.0", true, true, true).await;

    let consent_store = DynamoConsentStore::new(dynamo.clone(), table.clone());
    let event_queue = SqsEventQueue::new(sqs.clone(), url.clone());
    let handler = IngestHandler::new(consent_store, event_queue, NoOpEncryptor);

    let request = IngestEventRequest {
        consent_id: consent_id.clone(),
        event_type: "page_view".to_string(),
        payload: serde_json::json!({"url": "/test"}),
        user_id: None,
        jurisdiction: "EU".to_string(),
        policy_version: "v1.0".to_string(),
        purpose: "analytics".to_string(),
        sensitive_fields: None,
    };

    let err = handler.handle(request).await.unwrap_err();
    assert_eq!(err.status_code(), 403);

    cleanup_consent(&dynamo, &table, &consent_id).await;
}

#[tokio::test]
async fn integration_nonexistent_consent_returns_404() {
    if !should_run() {
        eprintln!("Skipping integration test (set INTEGRATION=1 to run)");
        return;
    }

    let config = make_aws_config().await;
    let dynamo = aws_sdk_dynamodb::Client::new(&config);
    let sqs = aws_sdk_sqs::Client::new(&config);

    let table = consent_table();
    let url = queue_url();

    let consent_store = DynamoConsentStore::new(dynamo, table);
    let event_queue = SqsEventQueue::new(sqs, url);
    let handler = IngestHandler::new(consent_store, event_queue, NoOpEncryptor);

    let request = IngestEventRequest {
        consent_id: Uuid::new_v4().to_string(),
        event_type: "page_view".to_string(),
        payload: serde_json::json!({"url": "/test"}),
        user_id: None,
        jurisdiction: "EU".to_string(),
        policy_version: "v1.0".to_string(),
        purpose: "analytics".to_string(),
        sensitive_fields: None,
    };

    let err = handler.handle(request).await.unwrap_err();
    assert_eq!(err.status_code(), 404);
}

#[tokio::test]
async fn integration_purpose_mismatch_rejected() {
    if !should_run() {
        eprintln!("Skipping integration test (set INTEGRATION=1 to run)");
        return;
    }

    let config = make_aws_config().await;
    let dynamo = aws_sdk_dynamodb::Client::new(&config);
    let sqs = aws_sdk_sqs::Client::new(&config);

    let table = consent_table();
    let url = queue_url();
    let consent_id = Uuid::new_v4().to_string();

    // analytics=true, marketing=false
    seed_consent(&dynamo, &table, &consent_id, "active", "v1.0", true, false, false).await;

    let consent_store = DynamoConsentStore::new(dynamo.clone(), table.clone());
    let event_queue = SqsEventQueue::new(sqs, url);
    let handler = IngestHandler::new(consent_store, event_queue, NoOpEncryptor);

    let request = IngestEventRequest {
        consent_id: consent_id.clone(),
        event_type: "promo_email".to_string(),
        payload: serde_json::json!({"campaign": "spring"}),
        user_id: None,
        jurisdiction: "EU".to_string(),
        policy_version: "v1.0".to_string(),
        purpose: "marketing".to_string(),
        sensitive_fields: None,
    };

    let err = handler.handle(request).await.unwrap_err();
    assert_eq!(err.status_code(), 403);

    cleanup_consent(&dynamo, &table, &consent_id).await;
}

#[tokio::test]
async fn integration_sensitive_field_encryption() {
    if !should_run() {
        eprintln!("Skipping integration test (set INTEGRATION=1 to run)");
        return;
    }

    let key_id = match kms_key_id() {
        Some(k) => k,
        None => {
            eprintln!("Skipping KMS test (set KMS_KEY_ID to run)");
            return;
        }
    };

    let config = make_aws_config().await;
    let dynamo = aws_sdk_dynamodb::Client::new(&config);
    let sqs = aws_sdk_sqs::Client::new(&config);
    let kms = aws_sdk_kms::Client::new(&config);

    let table = consent_table();
    let url = queue_url();
    let consent_id = Uuid::new_v4().to_string();

    seed_consent(&dynamo, &table, &consent_id, "active", "v1.0", true, true, true).await;
    purge_queue(&sqs, &url).await;

    let consent_store = DynamoConsentStore::new(dynamo.clone(), table.clone());
    let event_queue = SqsEventQueue::new(sqs.clone(), url.clone());
    let encryptor = KmsFieldEncryptor::new(kms, key_id);
    let handler = IngestHandler::new(consent_store, event_queue, encryptor);

    let request = IngestEventRequest {
        consent_id: consent_id.clone(),
        event_type: "profile_update".to_string(),
        payload: serde_json::json!({"email": "secret@example.com", "action": "update"}),
        user_id: None,
        jurisdiction: "EU".to_string(),
        policy_version: "v1.0".to_string(),
        purpose: "analytics".to_string(),
        sensitive_fields: Some(vec!["email".to_string()]),
    };

    let response = handler.handle(request).await.expect("should succeed");
    assert_eq!(response.status, "accepted");

    // Verify the SQS message has the email field encrypted (not plaintext)
    let body = receive_message(&sqs, &url).await.expect("expected SQS message");
    let msg: serde_json::Value = serde_json::from_str(&body).unwrap();
    let email_value = msg["payload"]["email"].as_str().unwrap();
    assert_ne!(email_value, "secret@example.com", "email should be encrypted");
    assert!(
        msg["encrypted_fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "email"),
        "encrypted_fields should list 'email'"
    );

    cleanup_consent(&dynamo, &table, &consent_id).await;
}

#[tokio::test]
async fn integration_policy_version_mismatch() {
    if !should_run() {
        eprintln!("Skipping integration test (set INTEGRATION=1 to run)");
        return;
    }

    let config = make_aws_config().await;
    let dynamo = aws_sdk_dynamodb::Client::new(&config);
    let sqs = aws_sdk_sqs::Client::new(&config);

    let table = consent_table();
    let url = queue_url();
    let consent_id = Uuid::new_v4().to_string();

    seed_consent(&dynamo, &table, &consent_id, "active", "v1.0", true, true, true).await;

    let consent_store = DynamoConsentStore::new(dynamo.clone(), table.clone());
    let event_queue = SqsEventQueue::new(sqs, url);
    let handler = IngestHandler::new(consent_store, event_queue, NoOpEncryptor);

    let request = IngestEventRequest {
        consent_id: consent_id.clone(),
        event_type: "page_view".to_string(),
        payload: serde_json::json!({"url": "/test"}),
        user_id: None,
        jurisdiction: "EU".to_string(),
        policy_version: "v99.0".to_string(), // mismatch
        purpose: "analytics".to_string(),
        sensitive_fields: None,
    };

    let err = handler.handle(request).await.unwrap_err();
    assert_eq!(err.status_code(), 409);

    cleanup_consent(&dynamo, &table, &consent_id).await;
}
