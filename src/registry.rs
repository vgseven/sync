//! PyPI and npm latest-version lookups.
//!
//! A single reusable HTTP client provides connection reuse. Callers pass a
//! bounded concurrency value so large manifests do not overwhelm registries or
//! the local network.

use crate::model::Ecosystem;
use anyhow::{Context, Result, bail};
use futures::{StreamExt, stream};
use reqwest::StatusCode;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub type LookupKey = (Ecosystem, String);
pub type LookupResults = HashMap<LookupKey, Result<String, String>>;

#[derive(Clone)]
pub struct RegistryClient {
    client: reqwest::Client,
    pypi_base: String,
    npm_base: String,
}

impl RegistryClient {
    /// Build the shared client once per command invocation.
    pub fn new(pypi_base: String, npm_base: String, timeout_seconds: u64) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout_seconds))
            .user_agent(format!("relay-sync/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            client,
            pypi_base: trim_trailing_slash(&pypi_base),
            npm_base: trim_trailing_slash(&npm_base),
        })
    }

    pub async fn fetch_many(&self, keys: Vec<LookupKey>, concurrency: usize) -> LookupResults {
        // buffer_unordered starts at most `concurrency` requests while allowing
        // faster registries/packages to complete without head-of-line blocking.
        stream::iter(keys.into_iter().map(|key| {
            let client = self.clone();
            async move {
                let result = client
                    .fetch_latest(key.0, &key.1)
                    .await
                    .map_err(|error| format!("{error:#}"));
                (key, result)
            }
        }))
        .buffer_unordered(concurrency)
        .collect()
        .await
    }

    async fn fetch_latest(&self, ecosystem: Ecosystem, name: &str) -> Result<String> {
        match ecosystem {
            Ecosystem::Python => self.fetch_pypi(name).await,
            Ecosystem::Node => self.fetch_npm(name).await,
        }
    }

    async fn fetch_pypi(&self, name: &str) -> Result<String> {
        let url = format!("{}/{}/json", self.pypi_base, encode_path_segment(name));
        let payload = self.get_json(&url).await?;
        payload
            .get("info")
            .and_then(|info| info.get("version"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .with_context(|| format!("PyPI response for {name} did not include info.version"))
    }

    async fn fetch_npm(&self, name: &str) -> Result<String> {
        let url = format!("{}/{}", self.npm_base, encode_path_segment(name));
        let payload = self.get_json(&url).await?;
        payload
            .get("dist-tags")
            .and_then(|tags| tags.get("latest"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .with_context(|| format!("npm response for {name} did not include dist-tags.latest"))
    }

    async fn get_json(&self, url: &str) -> Result<Value> {
        let response = self.client.get(url).send().await?;
        let status = response.status();
        if status == StatusCode::NOT_FOUND {
            bail!("registry package not found");
        }
        if !status.is_success() {
            bail!("registry returned HTTP {status}");
        }
        Ok(response.json::<Value>().await?)
    }
}

fn trim_trailing_slash(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}

fn encode_path_segment(value: &str) -> String {
    // Package names are one URL path segment. In particular, scoped npm names
    // must encode their slash instead of becoming a second path component.
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub fn normalize_lookup_name(ecosystem: Ecosystem, name: &str) -> String {
    // PyPI normalizes '-', '_' and '.' as equivalent; npm package names retain
    // their spelling because scoped names and case are registry-significant.
    match ecosystem {
        Ecosystem::Python => name
            .chars()
            .map(|character| match character {
                '_' | '.' => '-',
                other => other.to_ascii_lowercase(),
            })
            .collect(),
        Ecosystem::Node => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_scoped_npm_names() {
        assert_eq!(encode_path_segment("@scope/package"), "%40scope%2Fpackage");
    }

    #[test]
    fn normalizes_python_distribution_names() {
        assert_eq!(
            normalize_lookup_name(Ecosystem::Python, "My_Pkg.Name"),
            "my-pkg-name"
        );
    }
}
