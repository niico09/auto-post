//! Loads a project directory: `requests/*.json`, `workflows/*.json` and an
//! optional `globals.json`. The file stem is the request/workflow name.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use thiserror::Error;

use super::model::{RequestDef, Workflow};
use super::project::Project;
use super::validation::ValidationError;

/// Failure while reading or validating a project directory.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("cannot read `{}`: {source}", .path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON in `{}`: {source}", .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("`{}` must contain a JSON object", .path.display())]
    GlobalsNotObject { path: PathBuf },
    #[error(transparent)]
    Invalid(#[from] ValidationError),
}

/// Loads and validates the project rooted at `dir`.
///
/// `requests/` and `workflows/` must exist; `globals.json` is optional.
pub fn load_project(dir: &Path) -> Result<Project, ManifestError> {
    let requests: BTreeMap<String, RequestDef> = load_dir(&dir.join("requests"))?;
    let workflows: BTreeMap<String, Workflow> = load_dir(&dir.join("workflows"))?;
    let globals = load_globals(&dir.join("globals.json"))?;
    Ok(Project::from_parts(requests, workflows, globals)?)
}

fn load_dir<T: DeserializeOwned>(dir: &Path) -> Result<BTreeMap<String, T>, ManifestError> {
    let entries = fs::read_dir(dir).map_err(|source| io_error(dir, source))?;
    let mut loaded = BTreeMap::new();
    for entry in entries {
        let path = entry.map_err(|source| io_error(dir, source))?.path();
        let is_json = path.extension().is_some_and(|ext| ext == "json");
        let Some(stem) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .filter(|_| is_json)
        else {
            continue;
        };
        let value = read_json(&path)?;
        let parsed = serde_json::from_value(value).map_err(|source| ManifestError::Parse {
            path: path.clone(),
            source,
        })?;
        loaded.insert(stem.to_owned(), parsed);
    }
    Ok(loaded)
}

fn load_globals(path: &Path) -> Result<Map<String, Value>, ManifestError> {
    if !path.exists() {
        return Ok(Map::new());
    }
    match read_json(path)? {
        Value::Object(map) => Ok(map),
        _ => Err(ManifestError::GlobalsNotObject {
            path: path.to_owned(),
        }),
    }
}

fn read_json(path: &Path) -> Result<Value, ManifestError> {
    let text = fs::read_to_string(path).map_err(|source| io_error(path, source))?;
    serde_json::from_str(&text).map_err(|source| ManifestError::Parse {
        path: path.to_owned(),
        source,
    })
}

fn io_error(path: &Path, source: std::io::Error) -> ManifestError {
    ManifestError::Io {
        path: path.to_owned(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn write(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    const PING: &str = r#"{"method":"GET","url":"http://x"}"#;

    #[test]
    fn loads_a_project_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "requests/ping.json", PING);
        write(dir.path(), "requests/notes.txt", "ignored");
        write(
            dir.path(),
            "workflows/main.json",
            r#"{"steps":[{"id":"p","request":"ping"}]}"#,
        );
        write(dir.path(), "globals.json", r#"{"token":"abc"}"#);

        let project = load_project(dir.path()).unwrap();
        assert!(project.request("ping").is_some());
        assert!(project.workflow("main").is_some());
        assert_eq!(project.initial_globals().get("token"), Some(&json!("abc")));
    }

    #[test]
    fn globals_file_is_optional() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "requests/ping.json", PING);
        write(dir.path(), "workflows/main.json", r#"{"steps":[]}"#);
        assert!(load_project(dir.path())
            .unwrap()
            .initial_globals()
            .is_empty());
    }

    #[test]
    fn reports_parse_errors_with_the_file_path() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "requests/bad.json", "{ nope");
        write(dir.path(), "workflows/main.json", r#"{"steps":[]}"#);
        let err = load_project(dir.path()).unwrap_err();
        assert!(matches!(&err, ManifestError::Parse { path, .. } if path.ends_with("bad.json")));
    }

    #[test]
    fn surfaces_validation_errors() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "requests/ping.json", PING);
        write(
            dir.path(),
            "workflows/main.json",
            r#"{"steps":[{"id":"p","request":"missing"}]}"#,
        );
        assert!(matches!(
            load_project(dir.path()).unwrap_err(),
            ManifestError::Invalid(ValidationError::UnknownRequest { .. })
        ));
    }

    #[test]
    fn missing_directory_and_non_object_globals_are_errors() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load_project(dir.path()).unwrap_err(),
            ManifestError::Io { .. }
        ));
        write(dir.path(), "requests/a.json", PING);
        write(dir.path(), "workflows/a.json", r#"{"steps":[]}"#);
        write(dir.path(), "globals.json", "[1]");
        assert!(matches!(
            load_project(dir.path()).unwrap_err(),
            ManifestError::GlobalsNotObject { .. }
        ));
    }
}
