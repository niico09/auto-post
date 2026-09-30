//! Turning a request definition plus resolved inputs into an [`HttpRequest`],
//! and binding declared inputs.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::error::RunError;
use crate::http::HttpRequest;
use crate::manifest::{InputDef, RequestDef};
use crate::template::{resolve_str, resolve_value, Scopes};

/// Applies defaults and enforces the declared contract of `owner`
/// (a request or workflow name, used in error messages).
pub(super) fn bind_inputs(
    owner: &str,
    declared: &BTreeMap<String, InputDef>,
    mut provided: Map<String, Value>,
) -> Result<Map<String, Value>, RunError> {
    if let Some(input) = provided.keys().find(|key| !declared.contains_key(*key)) {
        return Err(RunError::UnknownInput {
            owner: owner.to_owned(),
            input: input.clone(),
        });
    }
    for (name, def) in declared {
        if provided.contains_key(name) {
            continue;
        }
        match &def.default {
            Some(default) => {
                provided.insert(name.clone(), default.clone());
            }
            None => {
                return Err(RunError::MissingInput {
                    owner: owner.to_owned(),
                    input: name.clone(),
                })
            }
        }
    }
    Ok(provided)
}

/// Resolves url, headers and body. Only the `inputs` scope is visible, so a
/// request can never read globals, locals or other steps.
pub(super) fn build_request(
    def: &RequestDef,
    inputs: &Map<String, Value>,
) -> Result<HttpRequest, RunError> {
    let scopes = Scopes::new().with("inputs", inputs);
    let headers = def
        .headers
        .iter()
        .map(|(name, value)| Ok((name.clone(), stringify(resolve_str(value, &scopes)?))))
        .collect::<Result<_, RunError>>()?;
    Ok(HttpRequest {
        method: def.method,
        url: stringify(resolve_str(&def.url, &scopes)?),
        headers,
        body: def
            .body
            .as_ref()
            .map(|body| resolve_value(body, &scopes))
            .transpose()?,
    })
}

fn stringify(value: Value) -> String {
    match value {
        Value::String(text) => text,
        other => other.to_string(),
    }
}
