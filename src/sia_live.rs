//! Feature-gated live Sia storage backend.
//!
//! This adapter keeps the core pipeline unchanged and uploads packed delta
//! bytes through an HTTP-backed Sia storage endpoint when the `sia-live`
//! feature is enabled.

use crate::error::{CoreSyncError, Result};
use crate::payload::DeltaPayload;
use crate::sia::{pack_delta_stream, StorageBackend, UploadReceipt};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::env;
use std::time::Duration;

/// Environment variable for the live storage base URL.
pub const SIA_STORAGE_BASE_URL_ENV: &str = "CORE_SYNC_SIA_STORAGE_URL";

/// Environment variable for the storage bearer token.
pub const SIA_STORAGE_TOKEN_ENV: &str = "CORE_SYNC_SIA_STORAGE_TOKEN";

/// Configuration for the live Sia storage adapter.
#[derive(Debug, Clone)]
pub struct SiaLiveConfig {
    /// Base URL of the live storage service.
    pub storage_base_url: String,
    /// Optional bearer token used for authentication.
    pub bearer_token: Option<String>,
}

impl SiaLiveConfig {
    /// Loads the live storage configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let storage_base_url = env::var(SIA_STORAGE_BASE_URL_ENV).map_err(|_| {
            CoreSyncError::Storage(format!(
                "missing environment variable {SIA_STORAGE_BASE_URL_ENV}"
            ))
        })?;

        let bearer_token = env::var(SIA_STORAGE_TOKEN_ENV)
            .ok()
            .filter(|v| !v.is_empty());

        Ok(Self {
            storage_base_url,
            bearer_token,
        })
    }
}

/// Live storage backend that uploads delta bytes to a real Sia endpoint.
#[derive(Debug, Clone)]
pub struct SiaStorageBackend {
    client: Client,
    config: SiaLiveConfig,
}

impl SiaStorageBackend {
    /// Creates a live backend from an explicit configuration.
    pub fn new(config: SiaLiveConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| CoreSyncError::Storage(e.to_string()))?;

        Ok(Self { client, config })
    }

    /// Creates a live backend from environment variables.
    pub fn from_env() -> Result<Self> {
        Self::new(SiaLiveConfig::from_env()?)
    }
}

#[derive(Debug, Deserialize)]
struct UploadAck {
    #[serde(default)]
    object_key: Option<String>,
    #[serde(default)]
    bytes_uploaded: Option<usize>,
    #[serde(default)]
    chunks_uploaded: Option<usize>,
}

impl StorageBackend for SiaStorageBackend {
    fn upload_delta(&self, object_key: &str, delta: &DeltaPayload) -> Result<UploadReceipt> {
        if delta.is_empty() {
            return Ok(UploadReceipt {
                object_key: object_key.to_string(),
                bytes_uploaded: 0,
                chunks_uploaded: 0,
            });
        }

        let packed = pack_delta_stream(delta);
        let url = format!(
            "{}/api/objects/{}/delta",
            self.config.storage_base_url.trim_end_matches('/'),
            encode_path_segment(object_key)
        );

        let mut request = self
            .client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .header("X-CoreSync-Object-Key", object_key)
            .body(packed.clone());

        if let Some(token) = &self.config.bearer_token {
            request = request.bearer_auth(token);
        }

        let response = request
            .send()
            .map_err(|e| CoreSyncError::Storage(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(CoreSyncError::Storage(format!(
                "storage upload failed with status {status}: {body}"
            )));
        }

        let body = response.text().unwrap_or_default();
        let upload_receipt = serde_json::from_str::<UploadAck>(&body)
            .map(|ack| UploadReceipt {
                object_key: ack.object_key.unwrap_or_else(|| object_key.to_string()),
                bytes_uploaded: ack.bytes_uploaded.unwrap_or(packed.len()),
                chunks_uploaded: ack.chunks_uploaded.unwrap_or(delta.len()),
            })
            .unwrap_or_else(|_| UploadReceipt {
                object_key: object_key.to_string(),
                bytes_uploaded: packed.len(),
                chunks_uploaded: delta.len(),
            });

        Ok(upload_receipt)
    }
}

fn encode_path_segment(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len());
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push_str(&format!("{byte:02X}"));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_encoding_escapes_reserved_bytes() {
        assert_eq!(encode_path_segment("data/file.bin"), "data%2Ffile.bin");
    }
}
