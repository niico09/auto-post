//! Extraction of values from an [`HttpResponse`] according to an [`Extractor`].

use serde_json::Value;
use thiserror::Error;

use crate::http::HttpResponse;
use crate::manifest::{ExtractSource, Extractor};
use crate::path::{self, PathError};

#[derive(Debug, Error)]
pub enum ExtractError {
    #[error("response body is not valid JSON: {0}")]
    BodyNotJson(#[source] serde_json::Error),
    #[error("body path `{0}` matched nothing")]
    BodyPathNotFound(String),
    #[error(transparent)]
    InvalidPath(#[from] PathError),
    #[error("header extractor requires `path` with the header name")]
    HeaderNameMissing,
    #[error("response has no header `{0}`")]
    HeaderNotFound(String),
}

/// Extracts one value. Body: `path` is `$.a.b[0].c` (omitted = whole body).
/// Header: `path` is the header name; the value is a string. Status: the
/// numeric status code.
pub fn extract(response: &HttpResponse, extractor: &Extractor) -> Result<Value, ExtractError> {
    match extractor.from {
        ExtractSource::Status => Ok(Value::from(response.status())),
        ExtractSource::Header => {
            let name = extractor
                .path
                .as_deref()
                .ok_or(ExtractError::HeaderNameMissing)?;
            response
                .header(name)
                .map(|value| Value::String(value.to_owned()))
                .ok_or_else(|| ExtractError::HeaderNotFound(name.to_owned()))
        }
        ExtractSource::Body => {
            let body = response.json_body().map_err(ExtractError::BodyNotJson)?;
            let expr = extractor.path.as_deref().unwrap_or("$");
            let segments = path::parse(expr)?;
            path::lookup(&body, &segments)
                .cloned()
                .ok_or_else(|| ExtractError::BodyPathNotFound(expr.to_owned()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn response() -> HttpResponse {
        HttpResponse::new(
            201,
            [("Authorization".to_owned(), "Bearer t".to_owned())],
            r#"{"data": {"items": [{"id": 7}, {"id": 8}], "ok": true}}"#,
        )
    }

    fn body(path: Option<&str>) -> Extractor {
        Extractor {
            from: ExtractSource::Body,
            path: path.map(str::to_owned),
        }
    }

    #[test]
    fn extracts_body_paths_keeping_types() {
        let r = response();
        assert_eq!(
            extract(&r, &body(Some("$.data.items[1].id"))).unwrap(),
            json!(8)
        );
        assert_eq!(extract(&r, &body(Some("$.data.ok"))).unwrap(), json!(true));
        assert_eq!(extract(&r, &body(None)).unwrap()["data"]["ok"], json!(true));
    }

    #[test]
    fn extracts_headers_and_status() {
        let r = response();
        let header = Extractor {
            from: ExtractSource::Header,
            path: Some("authorization".into()),
        };
        assert_eq!(extract(&r, &header).unwrap(), json!("Bearer t"));
        let status = Extractor {
            from: ExtractSource::Status,
            path: None,
        };
        assert_eq!(extract(&r, &status).unwrap(), json!(201));
    }

    #[test]
    fn reports_explicit_errors() {
        let r = response();
        assert!(matches!(
            extract(&r, &body(Some("$.data.nope"))),
            Err(ExtractError::BodyPathNotFound(p)) if p == "$.data.nope"
        ));
        assert!(matches!(
            extract(&r, &body(Some("$.a..b"))),
            Err(ExtractError::InvalidPath(_))
        ));
        let missing_header = Extractor {
            from: ExtractSource::Header,
            path: Some("x-none".into()),
        };
        assert!(matches!(
            extract(&r, &missing_header),
            Err(ExtractError::HeaderNotFound(_))
        ));
        let nameless = Extractor {
            from: ExtractSource::Header,
            path: None,
        };
        assert!(matches!(
            extract(&r, &nameless),
            Err(ExtractError::HeaderNameMissing)
        ));
        let text = HttpResponse::new(200, [], "plain");
        assert!(matches!(
            extract(&text, &body(Some("$.a"))),
            Err(ExtractError::BodyNotJson(_))
        ));
    }
}
