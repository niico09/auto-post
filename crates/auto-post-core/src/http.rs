//! HTTP transport abstraction. The runner depends on [`HttpClient`] only, so
//! tests inject fakes and the real transport ([`ReqwestClient`]) is swappable.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use thiserror::Error;

pub use crate::manifest::HttpMethod;

/// A fully resolved request, ready to send. `body` is sent as JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Value>,
}

/// A received response. Header names are stored lower-cased.
#[derive(Debug, Clone, PartialEq)]
pub struct HttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: String,
}

impl HttpResponse {
    pub fn new(
        status: u16,
        headers: impl IntoIterator<Item = (String, String)>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            status,
            headers: headers
                .into_iter()
                .map(|(name, value)| (name.to_ascii_lowercase(), value))
                .collect(),
            body: body.into(),
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    /// Case-insensitive header lookup.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn body_text(&self) -> &str {
        &self.body
    }

    /// Parses the body as JSON.
    pub fn json_body(&self) -> Result<Value, serde_json::Error> {
        serde_json::from_str(&self.body)
    }
}

/// Transport-level failure (no response was obtained).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum HttpError {
    #[error("request to `{url}` failed: {message}")]
    Transport { url: String, message: String },
}

/// Sends requests. Implementations must not interpret status codes: any
/// received response, including 4xx/5xx, is `Ok`.
#[async_trait]
pub trait HttpClient: Send + Sync {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Request timeout applied by [`ReqwestClient::default`].
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Production [`HttpClient`] backed by `reqwest`.
///
/// `Default` applies [`DEFAULT_TIMEOUT`] to the whole request (connect,
/// send and body), so a stalled server surfaces as [`HttpError::Transport`]
/// instead of hanging a workflow forever.
#[derive(Debug, Clone)]
pub struct ReqwestClient {
    client: reqwest::Client,
}

impl ReqwestClient {
    /// Wraps a preconfigured client (its own timeout settings apply).
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Builds a client that aborts any request exceeding `timeout`.
    pub fn with_timeout(timeout: Duration) -> Result<Self, HttpError> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|error| HttpError::Transport {
                url: String::new(),
                message: format!("cannot build HTTP client: {error}"),
            })?;
        Ok(Self { client })
    }
}

impl Default for ReqwestClient {
    fn default() -> Self {
        Self::with_timeout(DEFAULT_TIMEOUT).expect("default HTTP client configuration is valid")
    }
}

fn to_reqwest(method: HttpMethod) -> reqwest::Method {
    match method {
        HttpMethod::Get => reqwest::Method::GET,
        HttpMethod::Post => reqwest::Method::POST,
        HttpMethod::Put => reqwest::Method::PUT,
        HttpMethod::Patch => reqwest::Method::PATCH,
        HttpMethod::Delete => reqwest::Method::DELETE,
        HttpMethod::Head => reqwest::Method::HEAD,
    }
}

#[async_trait]
impl HttpClient for ReqwestClient {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let transport = |error: reqwest::Error| HttpError::Transport {
            url: request.url.clone(),
            message: error.to_string(),
        };
        let mut builder = self
            .client
            .request(to_reqwest(request.method), &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = &request.body {
            builder = builder.json(body);
        }
        let response = builder.send().await.map_err(transport)?;
        let status = response.status().as_u16();
        let headers = response.headers().iter().map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        });
        let headers: Vec<_> = headers.collect();
        let body = response.text().await.map_err(transport)?;
        Ok(HttpResponse::new(status, headers, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_is_case_insensitive() {
        let response = HttpResponse::new(200, [("X-Token".to_owned(), "abc".to_owned())], "{}");
        assert_eq!(response.header("x-token"), Some("abc"));
        assert_eq!(response.header("X-TOKEN"), Some("abc"));
        assert_eq!(response.header("other"), None);
    }

    #[test]
    fn default_timeout_is_thirty_seconds() {
        assert_eq!(DEFAULT_TIMEOUT, Duration::from_secs(30));
        let _ = ReqwestClient::default();
    }

    #[tokio::test]
    async fn stalled_server_times_out_as_transport_error() {
        // Accepts the connection but never answers.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let client = ReqwestClient::with_timeout(Duration::from_millis(150)).unwrap();
        let request = HttpRequest {
            method: HttpMethod::Get,
            url: url.clone(),
            headers: BTreeMap::new(),
            body: None,
        };
        let error = client.send(request).await.unwrap_err();
        assert!(matches!(&error, HttpError::Transport { url: u, .. } if *u == url));
        drop(listener);
    }

    #[test]
    fn json_body_parses_or_errors() {
        assert!(HttpResponse::new(200, [], r#"{"a":1}"#).json_body().is_ok());
        assert!(HttpResponse::new(200, [], "not json").json_body().is_err());
    }
}
