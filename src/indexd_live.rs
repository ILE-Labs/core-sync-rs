//! Feature-gated live indexd backend.
//!
//! This adapter reads and writes manifest metadata over HTTP when the
//! `sia-live` feature is enabled.

use crate::error::{CoreSyncError, Result};
use crate::indexd::{ManifestRecord, ManifestStore};
use crate::manifest::FileManifest;
use reqwest::blocking::Client;
use serde::Deserialize;
use std::env;
use std::time::Duration;

/// Environment variable for the live indexd base URL.
pub const INDEXD_BASE_URL_ENV: &str = "CORE_SYNC_INDEXD_URL";

/// Environment variable for the indexd bearer token.
pub const INDEXD_TOKEN_ENV: &str = "CORE_SYNC_INDEXD_TOKEN";

/// Configuration for the live indexd adapter.
#[derive(Debug, Clone)]
pub struct IndexdLiveConfig {
    /// Base URL of the live indexd service.
    pub indexd_base_url: String,
    /// Optional bearer token used for authentication.
    pub bearer_token: Option<String>,
}

impl IndexdLiveConfig {
    /// Loads the live indexd configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let indexd_base_url = env::var(INDEXD_BASE_URL_ENV).map_err(|_| {
            CoreSyncError::Indexd(format!(
                "missing environment variable {INDEXD_BASE_URL_ENV}"
            ))
        })?;

        let bearer_token = env::var(INDEXD_TOKEN_ENV).ok().filter(|v| !v.is_empty());

        Ok(Self {
            indexd_base_url,
            bearer_token,
        })
    }
}

/// Live manifest store that reads and writes object metadata via HTTP.
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
}

#[derive(Debug, Deserialize)]
struct ManifestBody {
    #[serde(default)]
    #[serde(rename = "version")]
    _version: Option<u32>,
    #[serde(default)]
    manifest: Option<FileManifest>,
}

impl ManifestStore for IndexdManifestStore {
    fn get_manifest(&self, object_key: &str) -> Result<Option<FileManifest>> {
        let url = endpoint(
            &self.config.indexd_base_url,
            &format!("api/objects/{}", encode_path_segment(object_key)),
        );

        let mut request = self.client.get(url);
        if let Some(token) = &self.config.bearer_token {
            request = request.bearer_auth(token);
        }

        let response = request
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
            return Ok(Some(record.manifest));
        }

        if let Ok(body) = serde_json::from_str::<ManifestBody>(&body) {
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
            &self.config.indexd_base_url,
            &format!("api/objects/{}", encode_path_segment(object_key)),
        );

        let body = ManifestRecord::new(manifest.clone())
            .to_json()
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;

        let mut request = self
            .client
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);

        if let Some(token) = &self.config.bearer_token {
            request = request.bearer_auth(token);
        }

        let response = request
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
