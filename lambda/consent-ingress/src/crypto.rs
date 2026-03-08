use async_trait::async_trait;
use aws_sdk_kms::primitives::Blob;
use aws_sdk_kms::Client as KmsClient;
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;

use crate::error::IngestionError;

#[async_trait]
pub trait FieldEncryptor: Send + Sync {
    async fn encrypt_field(&self, plaintext: &str) -> Result<String, IngestionError>;
}

pub struct KmsFieldEncryptor {
    client: KmsClient,
    key_id: String,
}

impl KmsFieldEncryptor {
    pub fn new(client: KmsClient, key_id: String) -> Self {
        Self { client, key_id }
    }
}

#[async_trait]
impl FieldEncryptor for KmsFieldEncryptor {
    async fn encrypt_field(&self, plaintext: &str) -> Result<String, IngestionError> {
        let result = self
            .client
            .encrypt()
            .key_id(&self.key_id)
            .plaintext(Blob::new(plaintext.as_bytes()))
            .send()
            .await
            .map_err(|e| IngestionError::EncryptionFailed {
                message: format!("KMS encrypt failed: {e}"),
            })?;

        let ciphertext = result.ciphertext_blob.ok_or_else(|| {
            IngestionError::EncryptionFailed {
                message: "KMS returned no ciphertext".to_string(),
            }
        })?;

        Ok(BASE64.encode(ciphertext.into_inner()))
    }
}

#[async_trait]
impl FieldEncryptor for Box<dyn FieldEncryptor> {
    async fn encrypt_field(&self, plaintext: &str) -> Result<String, IngestionError> {
        (**self).encrypt_field(plaintext).await
    }
}

pub struct NoOpEncryptor;

#[async_trait]
impl FieldEncryptor for NoOpEncryptor {
    async fn encrypt_field(&self, plaintext: &str) -> Result<String, IngestionError> {
        Ok(plaintext.to_string())
    }
}

pub async fn encrypt_sensitive_fields(
    encryptor: &dyn FieldEncryptor,
    payload: &mut serde_json::Value,
    sensitive_fields: &[String],
) -> Result<Vec<String>, IngestionError> {
    let obj = payload.as_object_mut().ok_or_else(|| {
        IngestionError::EncryptionFailed {
            message: "Payload is not an object".to_string(),
        }
    })?;

    let mut encrypted = Vec::new();

    for field in sensitive_fields {
        if let Some(value) = obj.get(field) {
            let plaintext = match value {
                serde_json::Value::String(s) => s.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            };

            let ciphertext = encryptor.encrypt_field(&plaintext).await?;
            obj.insert(field.clone(), serde_json::Value::String(ciphertext));
            encrypted.push(field.clone());
        }
    }

    Ok(encrypted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_noop_encryptor_returns_plaintext() {
        let enc = NoOpEncryptor;
        let result = enc.encrypt_field("hello").await.unwrap();
        assert_eq!(result, "hello");
    }

    #[tokio::test]
    async fn test_encrypt_sensitive_fields_replaces_values() {
        let enc = MockReverseEncryptor;
        let mut payload = serde_json::json!({
            "email": "user@example.com",
            "action": "click"
        });

        let encrypted = encrypt_sensitive_fields(
            &enc,
            &mut payload,
            &["email".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(encrypted, vec!["email"]);
        assert_eq!(
            payload["email"].as_str().unwrap(),
            "ENC:user@example.com"
        );
        assert_eq!(payload["action"].as_str().unwrap(), "click");
    }

    #[tokio::test]
    async fn test_encrypt_sensitive_fields_handles_non_string() {
        let enc = MockReverseEncryptor;
        let mut payload = serde_json::json!({
            "count": 42,
            "name": "test"
        });

        let encrypted = encrypt_sensitive_fields(
            &enc,
            &mut payload,
            &["count".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(encrypted, vec!["count"]);
        assert_eq!(payload["count"].as_str().unwrap(), "ENC:42");
    }

    #[tokio::test]
    async fn test_encrypt_sensitive_fields_skips_missing() {
        let enc = MockReverseEncryptor;
        let mut payload = serde_json::json!({"name": "test"});

        let encrypted = encrypt_sensitive_fields(
            &enc,
            &mut payload,
            &["nonexistent".to_string()],
        )
        .await
        .unwrap();

        assert!(encrypted.is_empty());
    }

    struct MockReverseEncryptor;

    #[async_trait]
    impl FieldEncryptor for MockReverseEncryptor {
        async fn encrypt_field(&self, plaintext: &str) -> Result<String, IngestionError> {
            Ok(format!("ENC:{plaintext}"))
        }
    }
}
