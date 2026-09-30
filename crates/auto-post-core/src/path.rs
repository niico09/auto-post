//! Tiny JSONPath-like path syntax shared by templates and extractors.
//!
//! Supported: `a.b`, `a[0].c`, `$.a.b[0].c` (a leading `$` is optional and
//! means "root"). On arrays, a key segment that is a number acts as an index
//! (`items.0`).

use serde_json::Value;
use thiserror::Error;

/// One step of a parsed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    Key(String),
    Index(usize),
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("invalid path `{path}`: {reason}")]
pub struct PathError {
    pub path: String,
    pub reason: &'static str,
}

/// Parses `path` into segments. `$` alone yields an empty list (the root).
pub fn parse(path: &str) -> Result<Vec<Segment>, PathError> {
    let raw = path.trim();
    let fail = |reason| PathError {
        path: raw.to_owned(),
        reason,
    };
    let (mut rest, rooted) = match raw.strip_prefix('$') {
        Some(tail) => (tail, true),
        None => (raw, false),
    };
    let mut segments = Vec::new();
    if !rooted {
        let (key, tail) = take_key(rest);
        if key.is_empty() || key.contains(']') {
            return Err(fail("empty or malformed path"));
        }
        segments.push(Segment::Key(key.to_owned()));
        rest = tail;
    }
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix('.') {
            let (key, tail) = take_key(tail);
            if key.is_empty() || key.contains(']') {
                return Err(fail("empty or malformed path segment"));
            }
            segments.push(Segment::Key(key.to_owned()));
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix('[') {
            let end = tail.find(']').ok_or_else(|| fail("unclosed `[`"))?;
            let index = tail[..end]
                .trim()
                .parse::<usize>()
                .map_err(|_| fail("array index must be a non-negative integer"))?;
            segments.push(Segment::Index(index));
            rest = &tail[end + 1..];
        } else {
            return Err(fail("unexpected character"));
        }
    }
    Ok(segments)
}

fn take_key(s: &str) -> (&str, &str) {
    let end = s.find(['.', '[']).unwrap_or(s.len());
    (s[..end].trim(), &s[end..])
}

/// Follows `segments` from `value`; `None` when any step is missing.
pub fn lookup<'a>(value: &'a Value, segments: &[Segment]) -> Option<&'a Value> {
    segments
        .iter()
        .try_fold(value, |current, segment| match (current, segment) {
            (Value::Object(map), Segment::Key(key)) => map.get(key),
            (Value::Array(items), Segment::Index(index)) => items.get(*index),
            (Value::Array(items), Segment::Key(key)) => {
                key.parse::<usize>().ok().and_then(|i| items.get(i))
            }
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn key(s: &str) -> Segment {
        Segment::Key(s.to_owned())
    }

    #[test]
    fn parses_dotted_and_indexed_paths() {
        assert_eq!(parse("a.b").unwrap(), vec![key("a"), key("b")]);
        assert_eq!(
            parse("$.a.b[0].c").unwrap(),
            vec![key("a"), key("b"), Segment::Index(0), key("c")]
        );
        assert_eq!(parse("$").unwrap(), vec![]);
        assert_eq!(parse("$[2]").unwrap(), vec![Segment::Index(2)]);
    }

    #[test]
    fn rejects_malformed_paths() {
        for bad in ["", "a..b", "a.", "a[", "a[x]", "a[-1]", "$a", "a]"] {
            assert!(parse(bad).is_err(), "`{bad}` should be rejected");
        }
    }

    #[test]
    fn looks_up_nested_values() {
        let doc = json!({"a": {"b": [{"c": 1}, {"c": 2}]}});
        assert_eq!(lookup(&doc, &parse("a.b[1].c").unwrap()), Some(&json!(2)));
        assert_eq!(lookup(&doc, &parse("a.b.0.c").unwrap()), Some(&json!(1)));
        assert_eq!(lookup(&doc, &parse("a.x").unwrap()), None);
        assert_eq!(lookup(&doc, &parse("a.b[5]").unwrap()), None);
        assert_eq!(lookup(&doc, &[]), Some(&doc));
    }
}
