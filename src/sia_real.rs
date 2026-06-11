//! Feature-gated live Sia storage backend.
//!
//! This adapter uploads delta chunk bytes to the live Sia HTTP API when the
//! `sia-live` feature is enabled.

use crate::error::{CoreSyncError, Result};
use crate::payload::DeltaPayload;
use crate::sia::{StorageBackend, UploadReceipt};
use reqwest::blocking::Client;
use std::env;
use std::time::Duration;

/// Environment variable for the live Sia API base URL.
pub const SIA_API_ENDPOINT_ENV: &str = "SIA_API_ENDPOINT";

/// Environment variable for the Sia API password.
pub const SIA_API_PASSWORD_ENV: &str = "SIA_API_PASSWORD";

/// Configuration for the live Sia adapter.
#[derive(Debug, Clone)]
pub struct SiaLiveConfig {
    /// Base URL of the live Sia API service.
    pub endpoint: String,
    /// Password used for HTTP basic authentication.
    pub api_password: String,
}

impl SiaLiveConfig {
    /// Loads the live Sia configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let endpoint = env::var(SIA_API_ENDPOINT_ENV).map_err(|_| {
            CoreSyncError::Storage(format!(
                "missing environment variable {SIA_API_ENDPOINT_ENV}"
            ))
        })?;
        let api_password = env::var(SIA_API_PASSWORD_ENV).map_err(|_| {
            CoreSyncError::Storage(format!(
                "missing environment variable {SIA_API_PASSWORD_ENV}"
            ))
        })?;

        Ok(Self {
            endpoint,
            api_password,
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

    /// Uploads a single chunk to the live Sia endpoint.
    pub fn upload_chunk(&self, chunk_hash: &str, data: &[u8]) -> Result<()> {
        let url = endpoint(
            &self.config.endpoint,
            &format!("chunks/{}", encode_path_segment(chunk_hash)),
        );
        let response = self
            .client
            .put(url)
            .basic_auth("sia", Some(&self.config.api_password))
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(data.to_vec())
            .send()
            .map_err(|e| CoreSyncError::Storage(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(CoreSyncError::Storage(format!(
                "chunk upload failed with status {status}: {body}"
            )));
        }

        Ok(())
    }

    /// Checks whether a chunk already exists on the live Sia endpoint.
    pub fn chunk_exists(&self, chunk_hash: &str) -> Result<bool> {
        let url = endpoint(
            &self.config.endpoint,
            &format!("chunks/{}", encode_path_segment(chunk_hash)),
        );
        let response = self
            .client
            .head(url)
            .basic_auth("sia", Some(&self.config.api_password))
            .send()
            .map_err(|e| CoreSyncError::Storage(e.to_string()))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(false);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(CoreSyncError::Storage(format!(
                "chunk existence check failed with status {status}: {body}"
            )));
        }

        Ok(true)
    }
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

        let mut bytes_uploaded = 0usize;
        let mut chunks_uploaded = 0usize;

        for chunk in &delta.chunks {
            if !self.chunk_exists(&chunk.hash)? {
                self.upload_chunk(&chunk.hash, &chunk.data)?;
                bytes_uploaded += chunk.data.len();
                chunks_uploaded += 1;
            }
        }

        Ok(UploadReceipt {
            object_key: object_key.to_string(),
            bytes_uploaded,
            chunks_uploaded,
        })
    }
}

fn endpoint(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
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
