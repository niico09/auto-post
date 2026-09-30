//! Command handlers. Output and HTTP transport are injected so they can be
//! tested without a process or a network.

use std::io::Write;

use auto_post_core::http::HttpClient;
use auto_post_core::manifest::{load_project, Project};
use auto_post_core::runner::Runner;
use serde_json::{Map, Value};

use crate::args::{KeyValue, RunArgs, ValidateArgs};
use crate::error::CliError;

/// Loads the project and reports `OK` with a summary on `out`.
pub fn validate(args: &ValidateArgs, out: &mut dyn Write) -> Result<(), CliError> {
    let project = load_project(&args.dir)?;
    writeln!(
        out,
        "OK: {} request(s), {} workflow(s)",
        project.request_names().count(),
        project.workflow_names().count()
    )?;
    Ok(())
}

/// Runs a workflow and writes its outputs as pretty JSON to `out`.
pub async fn run(
    args: &RunArgs,
    http: &dyn HttpClient,
    out: &mut dyn Write,
) -> Result<(), CliError> {
    let project = load_project(&args.dir)?;
    let outputs = execute(&project, args, http).await?;
    writeln!(out, "{}", serde_json::to_string_pretty(&outputs)?)?;
    Ok(())
}

async fn execute(
    project: &Project,
    args: &RunArgs,
    http: &dyn HttpClient,
) -> Result<Map<String, Value>, CliError> {
    let mut globals = project.initial_globals();
    globals.extend(to_map(&args.globals));
    let outputs = Runner::new(project, http)
        .run(&args.workflow, to_map(&args.inputs), &mut globals)
        .await?;
    Ok(outputs)
}

/// Later occurrences of a key win.
fn to_map(pairs: &[KeyValue]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|pair| (pair.key.clone(), pair.value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn pair(key: &str, value: Value) -> KeyValue {
        KeyValue {
            key: key.into(),
            value,
        }
    }

    #[test]
    fn later_pairs_override_earlier_ones() {
        let map = to_map(&[
            pair("a", json!(1)),
            pair("a", json!(2)),
            pair("b", json!("x")),
        ]);
        assert_eq!(Value::Object(map), json!({"a": 2, "b": "x"}));
    }
}
