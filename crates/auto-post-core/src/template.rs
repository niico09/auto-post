//! Variable scopes and `{{scope.path}}` template resolution.
//!
//! A whole-string placeholder (`"{{steps.login.token}}"`) yields the referenced
//! JSON value with its type intact; placeholders embedded in longer text are
//! stringified (strings verbatim, other values as compact JSON).

use serde_json::{Map, Value};
use thiserror::Error;

use crate::path::{self, PathError, Segment};

/// Failure while resolving a template. Always names the offending path.
#[derive(Debug, Error, PartialEq)]
pub enum TemplateError {
    #[error("unclosed `{{{{` in template `{0}`")]
    Unclosed(String),
    #[error("empty placeholder in template `{0}`")]
    EmptyPlaceholder(String),
    #[error("placeholder `{expr}`: {source}")]
    InvalidPath {
        expr: String,
        #[source]
        source: PathError,
    },
    #[error("variable `{path}` must be `<scope>.<name>[...]`")]
    NotAVariable { path: String },
    #[error("unknown or unavailable scope `{scope}` in `{path}`")]
    UnknownScope { scope: String, path: String },
    #[error("variable `{path}` is not defined")]
    Missing { path: String },
}

/// The named scopes visible to a template. Only the scopes added are
/// resolvable, which is how requests are kept away from globals.
#[derive(Debug, Default)]
pub struct Scopes<'a> {
    scopes: Vec<(&'static str, &'a Map<String, Value>)>,
}

impl<'a> Scopes<'a> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Makes `values` resolvable under `name` (e.g. `"globals"`).
    pub fn with(mut self, name: &'static str, values: &'a Map<String, Value>) -> Self {
        self.scopes.push((name, values));
        self
    }

    fn lookup(&self, expr: &str) -> Result<&'a Value, TemplateError> {
        let segments = path::parse(expr).map_err(|source| TemplateError::InvalidPath {
            expr: expr.to_owned(),
            source,
        })?;
        let not_a_variable = || TemplateError::NotAVariable {
            path: expr.to_owned(),
        };
        let [Segment::Key(scope), Segment::Key(name), rest @ ..] = segments.as_slice() else {
            return Err(not_a_variable());
        };
        let values = self
            .scopes
            .iter()
            .find(|(scope_name, _)| scope_name == scope)
            .map(|(_, values)| *values)
            .ok_or_else(|| TemplateError::UnknownScope {
                scope: scope.clone(),
                path: expr.to_owned(),
            })?;
        let missing = || TemplateError::Missing {
            path: expr.to_owned(),
        };
        let root = values.get(name).ok_or_else(missing)?;
        path::lookup(root, rest).ok_or_else(missing)
    }
}

enum Part<'t> {
    Literal(&'t str),
    Variable(&'t str),
}

fn split(template: &str) -> Result<Vec<Part<'_>>, TemplateError> {
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        if start > 0 {
            parts.push(Part::Literal(&rest[..start]));
        }
        let after = &rest[start + 2..];
        let end = after
            .find("}}")
            .ok_or_else(|| TemplateError::Unclosed(template.to_owned()))?;
        let expr = after[..end].trim();
        if expr.is_empty() {
            return Err(TemplateError::EmptyPlaceholder(template.to_owned()));
        }
        parts.push(Part::Variable(expr));
        rest = &after[end + 2..];
    }
    if !rest.is_empty() {
        parts.push(Part::Literal(rest));
    }
    Ok(parts)
}

/// Resolves the placeholders of one string.
pub fn resolve_str(template: &str, scopes: &Scopes<'_>) -> Result<Value, TemplateError> {
    let parts = split(template)?;
    if let [Part::Variable(expr)] = parts.as_slice() {
        return scopes.lookup(expr).cloned();
    }
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Literal(text) => out.push_str(text),
            Part::Variable(expr) => match scopes.lookup(expr)? {
                Value::String(text) => out.push_str(text),
                other => out.push_str(&other.to_string()),
            },
        }
    }
    Ok(Value::String(out))
}

/// Recursively resolves every string inside `value` (object keys are kept).
pub fn resolve_value(value: &Value, scopes: &Scopes<'_>) -> Result<Value, TemplateError> {
    match value {
        Value::String(text) => resolve_str(text, scopes),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_value(item, scopes))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(map) => resolve_map(map.iter(), scopes).map(Value::Object),
        other => Ok(other.clone()),
    }
}

/// Resolves the values of name/value pairs into a JSON object map.
pub fn resolve_map<'v>(
    entries: impl IntoIterator<Item = (&'v String, &'v Value)>,
    scopes: &Scopes<'_>,
) -> Result<Map<String, Value>, TemplateError> {
    entries
        .into_iter()
        .map(|(name, value)| Ok((name.clone(), resolve_value(value, scopes)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn whole_placeholder_keeps_json_type() {
        let globals = map(json!({"count": 3, "flag": true, "obj": {"a": [1, 2]}}));
        let scopes = Scopes::new().with("globals", &globals);
        assert_eq!(resolve_str("{{globals.count}}", &scopes), Ok(json!(3)));
        assert_eq!(
            resolve_str("  {{ globals.flag }}", &scopes),
            Ok(json!("  true"))
        );
        assert_eq!(resolve_str("{{globals.obj.a[1]}}", &scopes), Ok(json!(2)));
        assert_eq!(
            resolve_str("{{globals.obj}}", &scopes),
            Ok(json!({"a": [1, 2]}))
        );
    }

    #[test]
    fn embedded_placeholders_are_stringified() {
        let inputs = map(json!({"name": "ada", "n": 7, "tags": ["a"]}));
        let scopes = Scopes::new().with("inputs", &inputs);
        assert_eq!(
            resolve_str("Hi {{inputs.name}} #{{inputs.n}} {{inputs.tags}}", &scopes),
            Ok(json!("Hi ada #7 [\"a\"]"))
        );
        assert_eq!(resolve_str("plain", &scopes), Ok(json!("plain")));
    }

    #[test]
    fn resolves_nested_structures_and_leaves_non_strings() {
        let inputs = map(json!({"id": 5}));
        let scopes = Scopes::new().with("inputs", &inputs);
        let body = json!({"a": ["{{inputs.id}}", 1, null], "b": {"c": "x{{inputs.id}}"}});
        assert_eq!(
            resolve_value(&body, &scopes),
            Ok(json!({"a": [5, 1, null], "b": {"c": "x5"}}))
        );
    }

    #[test]
    fn missing_variable_names_the_path() {
        let inputs = map(json!({"a": {}}));
        let scopes = Scopes::new().with("inputs", &inputs);
        assert_eq!(
            resolve_str("{{inputs.a.b}}", &scopes),
            Err(TemplateError::Missing {
                path: "inputs.a.b".into()
            })
        );
        assert_eq!(
            resolve_str("x {{inputs.zzz}}", &scopes),
            Err(TemplateError::Missing {
                path: "inputs.zzz".into()
            })
        );
    }

    #[test]
    fn unavailable_scope_is_an_explicit_error() {
        let inputs = map(json!({"a": 1}));
        let scopes = Scopes::new().with("inputs", &inputs);
        assert_eq!(
            resolve_str("{{globals.token}}", &scopes),
            Err(TemplateError::UnknownScope {
                scope: "globals".into(),
                path: "globals.token".into()
            })
        );
    }

    #[test]
    fn malformed_templates_are_rejected() {
        let scopes = Scopes::new();
        assert!(matches!(
            resolve_str("{{a.b", &scopes),
            Err(TemplateError::Unclosed(_))
        ));
        assert!(matches!(
            resolve_str("{{  }}", &scopes),
            Err(TemplateError::EmptyPlaceholder(_))
        ));
        assert!(matches!(
            resolve_str("{{inputs}}", &scopes),
            Err(TemplateError::NotAVariable { .. })
        ));
        assert!(matches!(
            resolve_str("{{a..b}}", &scopes),
            Err(TemplateError::InvalidPath { .. })
        ));
    }
}
