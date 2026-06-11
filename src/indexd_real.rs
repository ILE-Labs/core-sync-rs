//! Feature-gated live indexd backend.
//!
//! This adapter persists file manifests through the live indexd HTTP API when
//! the `sia-live` feature is enabled.

use crate::error::{CoreSyncError, Result};
use crate::indexd::{ManifestRecord, ManifestStore};
use crate::manifest::FileManifest;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::env;
use std::time::Duration;

/// Environment variable for the live indexd base URL.
pub const INDEXD_ENDPOINT_ENV: &str = "INDEXD_ENDPOINT";

/// Environment variable for the indexd API key.
pub const INDEXD_API_KEY_ENV: &str = "INDEXD_API_KEY";

/// Configuration for the live indexd adapter.
#[derive(Debug, Clone)]
pub struct IndexdLiveConfig {
    /// Base URL of the live indexd service.
    pub endpoint: String,
    /// API key used for authentication.
    pub api_key: String,
}

impl IndexdLiveConfig {
    /// Loads the live indexd configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let endpoint = env::var(INDEXD_ENDPOINT_ENV).map_err(|_| {
            CoreSyncError::Indexd(format!(
                "missing environment variable {INDEXD_ENDPOINT_ENV}"
            ))
        })?;
        let api_key = env::var(INDEXD_API_KEY_ENV).map_err(|_| {
            CoreSyncError::Indexd(format!("missing environment variable {INDEXD_API_KEY_ENV}"))
        })?;

        Ok(Self { endpoint, api_key })
    }
}

/// Live manifest store backed by indexd.
#[derive(Debug, Clone)]
pub struct IndexdManifestStore {
    client: Client,
    config: IndexdLiveConfig,
}

impl IndexdManifestStore {
    /// Creates a live manifest store from an explicit configuration.
    pub fn new(config: IndexdLiveConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;

        Ok(Self { client, config })
    }

    /// Creates a live manifest store from environment variables.
    pub fn from_env() -> Result<Self> {
        Self::new(IndexdLiveConfig::from_env()?)
    }

    fn auth(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        request.header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", self.config.api_key),
        )
    }
}

#[derive(Debug, Deserialize)]
struct ManifestBody {
    #[serde(default)]
    manifest: Option<FileManifest>,
    #[serde(default)]
    record: Option<ManifestRecord>,
}

impl ManifestStore for IndexdManifestStore {
    fn get_manifest(&self, object_key: &str) -> Result<Option<FileManifest>> {
        let url = endpoint(
            &self.config.endpoint,
            &format!("manifests/{}", encode_path_segment(object_key)),
        );
        let response = self
            .auth(self.client.get(url))
            .send()
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(CoreSyncError::Indexd(format!(
                "manifest lookup failed with status {status}: {body}"
            )));
        }

        let body = response
            .text()
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;

        if let Ok(record) = ManifestRecord::from_json(&body) {
            record.manifest.validate()?;
            return Ok(Some(record.manifest));
        }

        if let Ok(body) = serde_json::from_str::<ManifestBody>(&body) {
            if let Some(record) = body.record {
                record.manifest.validate()?;
                return Ok(Some(record.manifest));
            }
            if let Some(manifest) = body.manifest {
                manifest.validate()?;
                return Ok(Some(manifest));
            }
        }

        let manifest = serde_json::from_str::<FileManifest>(&body)
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;
        manifest.validate()?;
        Ok(Some(manifest))
    }

    fn put_manifest(&self, object_key: &str, manifest: &FileManifest) -> Result<()> {
        manifest.validate()?;

        let url = endpoint(
            &self.config.endpoint,
            &format!("manifests/{}", encode_path_segment(object_key)),
        );
        let body = ManifestRecord::new(manifest.clone())
            .to_json()
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;

        let response = self
            .auth(
                self.client
                    .put(url)
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(body),
            )
            .send()
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().unwrap_or_default();
            return Err(CoreSyncError::Indexd(format!(
                "manifest publish failed with status {status}: {body}"
            )));
        }

        Ok(())
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
        assert_eq!(
            encode_path_segment("coresync:manifest"),
            "coresync%3Amanifest"
        );
    }
}
