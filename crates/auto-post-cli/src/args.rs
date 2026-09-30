//! Command line definition.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde_json::Value;

/// Run JSON-manifest HTTP workflows.
#[derive(Debug, Parser)]
#[command(name = "auto-post", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Load a project and check all manifests; prints OK or the first error.
    Validate(ValidateArgs),
    /// Run a workflow and print its outputs as pretty JSON on stdout.
    Run(RunArgs),
}

#[derive(Debug, Args)]
pub struct ValidateArgs {
    /// Project directory (contains requests/, workflows/, globals.json).
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,
}

#[derive(Debug, Args)]
pub struct RunArgs {
    /// Name of the workflow to run (file stem in workflows/).
    pub workflow: String,
    /// Project directory (contains requests/, workflows/, globals.json).
    #[arg(long, default_value = ".")]
    pub dir: PathBuf,
    /// Workflow input, `KEY=VALUE`. Repeatable. VALUE is parsed as JSON when
    /// valid, otherwise used as a string.
    #[arg(long = "input", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    pub inputs: Vec<KeyValue>,
    /// Override a value from globals.json, `KEY=VALUE`. Repeatable. VALUE is
    /// parsed as JSON when valid, otherwise used as a string.
    #[arg(long = "global", value_name = "KEY=VALUE", value_parser = parse_key_value)]
    pub globals: Vec<KeyValue>,
    /// Log each request and its status on stderr (secrets masked).
    #[arg(long)]
    pub verbose: bool,
}

/// A `KEY=VALUE` pair with the value already converted to JSON.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyValue {
    pub key: String,
    pub value: Value,
}

/// Parses `KEY=VALUE`. The value is JSON when it parses as such (`42`,
/// `true`, `{"a":1}`), otherwise the raw text as a string.
pub fn parse_key_value(raw: &str) -> Result<KeyValue, String> {
    let (key, value) = raw
        .split_once('=')
        .ok_or_else(|| format!("expected KEY=VALUE, got `{raw}`"))?;
    if key.is_empty() {
        return Err(format!("empty key in `{raw}`"));
    }
    let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_owned()));
    Ok(KeyValue {
        key: key.to_owned(),
        value,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_json_values_and_falls_back_to_strings() {
        assert_eq!(parse_key_value("n=42").unwrap().value, json!(42));
        assert_eq!(parse_key_value("b=true").unwrap().value, json!(true));
        assert_eq!(
            parse_key_value(r#"o={"a":1}"#).unwrap().value,
            json!({"a": 1})
        );
        assert_eq!(
            parse_key_value("url=https://x.io/a=b").unwrap().value,
            json!("https://x.io/a=b")
        );
        assert_eq!(parse_key_value("empty=").unwrap().value, json!(""));
    }

    #[test]
    fn rejects_missing_separator_and_empty_key() {
        assert!(parse_key_value("novalue").is_err());
        assert!(parse_key_value("=x").is_err());
    }

    #[test]
    fn parses_run_command() {
        let cli = Cli::try_parse_from([
            "auto-post",
            "run",
            "profile",
            "--dir",
            "p",
            "--input",
            "a=1",
            "--global",
            "g=x",
            "--verbose",
        ])
        .unwrap();
        let Command::Run(run) = cli.command else {
            panic!("expected run");
        };
        assert_eq!(run.workflow, "profile");
        assert_eq!(run.inputs.len(), 1);
        assert_eq!(run.globals[0].key, "g");
        assert!(run.verbose);
    }

    #[test]
    fn run_requires_a_workflow_name() {
        assert!(Cli::try_parse_from(["auto-post", "run"]).is_err());
    }
}
