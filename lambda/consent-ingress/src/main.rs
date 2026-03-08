use std::sync::Arc;

use lambda_http::{run, service_fn, Body, Error, Request, Response};
use tracing::info;

use consent_ingress::consent::DynamoConsentStore;
use consent_ingress::crypto::{FieldEncryptor, KmsFieldEncryptor, NoOpEncryptor};
use consent_ingress::error::IngestionError;
use consent_ingress::handler::IngestHandler;
use consent_ingress::models::IngestEventRequest;
use consent_ingress::queue::SqsEventQueue;

struct AppConfig {
    consent_table: String,
    queue_url: String,
    kms_key_id: Option<String>,
}

impl AppConfig {
    fn from_env() -> Self {
        Self {
            consent_table: std::env::var("CONSENT_TABLE_NAME")
                .unwrap_or_else(|_| "yourdata-consents".to_string()),
            queue_url: std::env::var("EVENT_QUEUE_URL")
                .expect("EVENT_QUEUE_URL environment variable is required"),
            kms_key_id: std::env::var("KMS_KEY_ID").ok(),
        }
    }
}

fn build_error_response(err: &IngestionError) -> Response<Body> {
    let api_err = err.to_api_response();
    let status = err.status_code();
    let body = serde_json::to_string(&api_err).unwrap_or_else(|_| {
        r#"{"error_code":"INTERNAL_ERROR","message":"Failed to serialize error"}"#.to_string()
    });

    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(body))
        .expect("failed to build error response")
}

fn build_success_response<T: serde::Serialize>(status: u16, body: &T) -> Response<Body> {
    let json = serde_json::to_string(body).expect("failed to serialize response");
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(json))
        .expect("failed to build success response")
}

async fn handle_lambda(
    handler: &IngestHandler<DynamoConsentStore, SqsEventQueue, Box<dyn FieldEncryptor>>,
    event: Request,
) -> Result<Response<Body>, Error> {
    let body = match event.body() {
        Body::Text(text) => text.clone(),
        Body::Binary(bytes) => String::from_utf8_lossy(bytes).to_string(),
        Body::Empty => {
            return Ok(build_error_response(&IngestionError::SchemaValidation {
                message: "Request body is empty".to_string(),
            }));
        }
    };

    let request: IngestEventRequest = match serde_json::from_str(&body) {
        Ok(req) => req,
        Err(e) => {
            return Ok(build_error_response(&IngestionError::SchemaValidation {
                message: format!("Invalid JSON: {e}"),
            }));
        }
    };

    info!(
        consent_id = %request.consent_id,
        event_type = %request.event_type,
        purpose = %request.purpose,
        "Processing ingestion request"
    );

    match handler.handle(request).await {
        Ok(response) => {
            info!(
                event_id = %response.event_id,
                correlation_id = %response.correlation_id,
                "Event accepted and enqueued"
            );
            Ok(build_success_response(202, &response))
        }
        Err(err) => {
            let code = err.error_code();
            let status = err.status_code();
            info!(
                error_code = ?code,
                status = status,
                "Ingestion rejected"
            );
            Ok(build_error_response(&err))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .json()
        .with_target(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = AppConfig::from_env();
    let aws_config = aws_config::load_defaults(aws_config::BehaviorVersion::latest()).await;

    let dynamo_client = aws_sdk_dynamodb::Client::new(&aws_config);
    let sqs_client = aws_sdk_sqs::Client::new(&aws_config);

    let consent_store = DynamoConsentStore::new(dynamo_client, config.consent_table);
    let event_queue = SqsEventQueue::new(sqs_client, config.queue_url);

    let encryptor: Box<dyn FieldEncryptor> = if let Some(ref key_id) = config.kms_key_id {
        let kms_client = aws_sdk_kms::Client::new(&aws_config);
        Box::new(KmsFieldEncryptor::new(kms_client, key_id.clone()))
    } else {
        info!("KMS_KEY_ID not set; sensitive field encryption disabled");
        Box::new(NoOpEncryptor)
    };

    let handler = Arc::new(IngestHandler::new(consent_store, event_queue, encryptor));

    run(service_fn(move |event: Request| {
        let handler = Arc::clone(&handler);
        async move { handle_lambda(&handler, event).await }
    }))
    .await
}
