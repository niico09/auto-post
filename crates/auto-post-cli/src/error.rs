//! Errors surfaced by CLI commands (all map to exit code 1).

use auto_post_core::manifest::ManifestError;
use auto_post_core::runner::RunError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliError {
    #[error("{0}")]
    Manifest(#[from] ManifestError),
    #[error("{0}")]
    Run(#[from] RunError),
    #[error("cannot write output: {0}")]
    Output(#[from] std::io::Error),
    #[error("cannot serialize output: {0}")]
    Serialize(#[from] serde_json::Error),
}
