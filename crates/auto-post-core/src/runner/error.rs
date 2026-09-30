//! Errors raised while running workflows.

use thiserror::Error;

use crate::extract::ExtractError;
use crate::http::HttpError;
use crate::template::TemplateError;

#[derive(Debug, Error)]
pub enum RunError {
    #[error("unknown workflow `{0}`")]
    UnknownWorkflow(String),
    #[error("unknown request `{0}`")]
    UnknownRequest(String),
    #[error("missing required input `{input}` for `{owner}`")]
    MissingInput { owner: String, input: String },
    #[error("`{owner}` declares no input `{input}`")]
    UnknownInput { owner: String, input: String },
    #[error(transparent)]
    Template(#[from] TemplateError),
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("cannot extract output `{output}`: {source}")]
    Extract {
        output: String,
        #[source]
        source: ExtractError,
    },
    #[error("request `{request}` returned unexpected status {status}")]
    UnexpectedStatus { request: String, status: u16 },
    #[error(
        "request `{request}` still returned status {status} after {max} recovery attempt(s) with workflow `{run}`"
    )]
    RecoveryExhausted {
        request: String,
        status: u16,
        run: String,
        max: u32,
    },
    #[error("recovery workflow `{run}` for status {status} failed: {source}")]
    RecoveryFailed {
        run: String,
        status: u16,
        #[source]
        source: Box<RunError>,
    },
    #[error("workflow `{workflow}`, step `{step}`: {source}")]
    InStep {
        workflow: String,
        step: String,
        #[source]
        source: Box<RunError>,
    },
}

impl RunError {
    /// Strips the step/recovery context wrappers and returns the underlying error.
    pub fn root_cause(&self) -> &RunError {
        match self {
            RunError::InStep { source, .. } | RunError::RecoveryFailed { source, .. } => {
                source.root_cause()
            }
            other => other,
        }
    }
}
