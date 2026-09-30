//! Verbose logging decorator for any [`HttpClient`].

use async_trait::async_trait;
use auto_post_core::http::{HttpClient, HttpError, HttpRequest, HttpResponse};

/// Header names whose values are never logged.
const SENSITIVE_HEADERS: [&str; 4] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
];

const MASK: &str = "***";

/// Value to display for a header, masked when it may carry credentials.
pub fn display_header_value<'a>(name: &str, value: &'a str) -> &'a str {
    if SENSITIVE_HEADERS
        .iter()
        .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
    {
        MASK
    } else {
        value
    }
}

/// Logs every request and its status to `sink`, then delegates to `inner`.
/// A recovery shows up as the recovery workflow's requests followed by a
/// repeat of the request that returned the recovered status.
pub struct LoggingClient<C, S> {
    inner: C,
    sink: S,
}

impl<C, S> LoggingClient<C, S>
where
    S: Fn(&str) + Send + Sync,
{
    pub fn new(inner: C, sink: S) -> Self {
        Self { inner, sink }
    }
}

#[async_trait]
impl<C, S> HttpClient for LoggingClient<C, S>
where
    C: HttpClient,
    S: Fn(&str) + Send + Sync,
{
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let headers: Vec<String> = request
            .headers
            .iter()
            .map(|(name, value)| format!("{name}={}", display_header_value(name, value)))
            .collect();
        let method = format!("{:?}", request.method).to_uppercase();
        let label = format!("{method} {}", request.url);
        (self.sink)(&format!("-> {label} [{}]", headers.join(", ")));
        let result = self.inner.send(request).await;
        match &result {
            Ok(response) => (self.sink)(&format!("<- {} {label}", response.status())),
            Err(error) => (self.sink)(&format!("<- error: {error}")),
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use auto_post_core::http::HttpMethod;

    use super::*;

    struct Fixed(u16);

    #[async_trait]
    impl HttpClient for Fixed {
        async fn send(&self, _: HttpRequest) -> Result<HttpResponse, HttpError> {
            Ok(HttpResponse::new(self.0, [], "{}"))
        }
    }

    #[tokio::test]
    async fn logs_request_and_status_with_masked_secrets() {
        let lines = Mutex::new(Vec::<String>::new());
        let client = LoggingClient::new(Fixed(401), |line: &str| {
            lines.lock().unwrap().push(line.to_owned());
        });
        let headers = BTreeMap::from([
            ("Authorization".to_owned(), "Bearer secret".to_owned()),
            ("Cookie".to_owned(), "sid=secret".to_owned()),
            ("X-Trace".to_owned(), "abc".to_owned()),
        ]);
        client
            .send(HttpRequest {
                method: HttpMethod::Get,
                url: "http://api/me".into(),
                headers,
                body: None,
            })
            .await
            .unwrap();

        let lines = lines.into_inner().unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("-> GET http://api/me"));
        assert!(lines[0].contains("Authorization=***"));
        assert!(lines[0].contains("Cookie=***"));
        assert!(lines[0].contains("X-Trace=abc"));
        assert!(!lines.join("\n").contains("secret"));
        assert!(lines[1].starts_with("<- 401"));
    }
}
