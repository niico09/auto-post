//! Serde types describing request definitions, workflows and steps.
//!
//! These are plain data types; behaviour lives in the loader, validator and
//! runner. A [`Step`] can only be deserialized into a valid shape (see
//! [`StepShapeError`]), so downstream code never handles "neither request nor
//! workflow" steps.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

/// HTTP verbs supported by request definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
}

/// A declared input of a request or workflow.
///
/// An input without a `default` is required. A JSON `null` default is
/// indistinguishable from "no default" and therefore counts as required.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputDef {
    #[serde(default)]
    pub default: Option<Value>,
}

impl InputDef {
    pub fn is_required(&self) -> bool {
        self.default.is_none()
    }
}

/// Where an [`Extractor`] reads its value from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtractSource {
    /// Parsed JSON body; `path` is a `$.a.b[0]` expression (omitted = whole body).
    Body,
    /// Response header; `path` is the header name (case-insensitive).
    Header,
    /// HTTP status code; `path` is ignored.
    Status,
}

/// Declares how to pull one value out of an HTTP response.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Extractor {
    pub from: ExtractSource,
    #[serde(default)]
    pub path: Option<String>,
}

/// A reusable HTTP request with a declared input/output contract.
///
/// Templates inside can only reference `{{inputs.*}}`; requests never see
/// globals, locals or other steps.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestDef {
    pub method: HttpMethod,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub inputs: BTreeMap<String, InputDef>,
    #[serde(default)]
    pub outputs: BTreeMap<String, Extractor>,
}

/// What to do once a recovery workflow finished.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecoveryAction {
    /// Re-run the failed step (re-resolving its bindings) and continue from it.
    #[default]
    Retry,
}

/// Recovery policy for one HTTP status of a step, e.g. re-login on 401.
///
/// The recovery workflow is run without inputs, so it must not declare
/// required ones. It may change globals through `set_global` steps.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecoveryPolicy {
    pub run: String,
    #[serde(default)]
    pub then: RecoveryAction,
    #[serde(default = "default_max_recoveries")]
    pub max: u32,
}

fn default_max_recoveries() -> u32 {
    1
}

/// What a step executes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepTarget {
    Request(String),
    Workflow(String),
}

/// Invalid combination of step fields.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum StepShapeError {
    #[error("step `{0}` must define exactly one of `request` or `workflow`, found both")]
    BothTargets(String),
    #[error("step `{0}` must define exactly one of `request` or `workflow`, found neither")]
    NoTarget(String),
    #[error("step `{0}` calls a workflow, so `extract` and `on_status` are not allowed")]
    RequestOnlyFieldOnWorkflow(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawStep {
    id: String,
    #[serde(default)]
    request: Option<String>,
    #[serde(default)]
    workflow: Option<String>,
    #[serde(default)]
    with: BTreeMap<String, Value>,
    #[serde(default)]
    extract: BTreeMap<String, Extractor>,
    #[serde(default)]
    set_global: BTreeMap<String, Value>,
    #[serde(default)]
    on_status: BTreeMap<u16, RecoveryPolicy>,
}

/// One step of a workflow.
///
/// * `with`: input bindings; values may be `{{scope.path}}` templates over
///   `globals`, `locals`, `steps` and the workflow's `inputs`.
/// * `extract`: extra step outputs (request steps only), overriding the
///   request's declared outputs of the same name.
/// * `set_global`: after the step succeeds, each value is resolved (the step's
///   own outputs are visible as `steps.<id>.*`) and stored in globals.
/// * `on_status`: recovery policies keyed by HTTP status (request steps only).
#[derive(Debug, Clone, Deserialize)]
#[serde(try_from = "RawStep")]
pub struct Step {
    pub id: String,
    pub target: StepTarget,
    pub with: BTreeMap<String, Value>,
    pub extract: BTreeMap<String, Extractor>,
    pub set_global: BTreeMap<String, Value>,
    pub on_status: BTreeMap<u16, RecoveryPolicy>,
}

impl TryFrom<RawStep> for Step {
    type Error = StepShapeError;

    fn try_from(raw: RawStep) -> Result<Self, Self::Error> {
        let target = match (raw.request, raw.workflow) {
            (Some(_), Some(_)) => return Err(StepShapeError::BothTargets(raw.id)),
            (None, None) => return Err(StepShapeError::NoTarget(raw.id)),
            (Some(request), None) => StepTarget::Request(request),
            (None, Some(workflow)) => {
                if !raw.extract.is_empty() || !raw.on_status.is_empty() {
                    return Err(StepShapeError::RequestOnlyFieldOnWorkflow(raw.id));
                }
                StepTarget::Workflow(workflow)
            }
        };
        Ok(Step {
            id: raw.id,
            target,
            with: raw.with,
            extract: raw.extract,
            set_global: raw.set_global,
            on_status: raw.on_status,
        })
    }
}

impl Step {
    /// Names of workflows this step depends on (called or used for recovery).
    pub fn referenced_workflows(&self) -> impl Iterator<Item = &str> {
        let called = match &self.target {
            StepTarget::Workflow(name) => Some(name.as_str()),
            StepTarget::Request(_) => None,
        };
        called
            .into_iter()
            .chain(self.on_status.values().map(|policy| policy.run.as_str()))
    }
}

/// An ordered list of steps with its own inputs, locals and outputs.
///
/// `outputs` values are templates resolved after the last step; they are what
/// a calling step receives as its outputs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workflow {
    #[serde(default)]
    pub inputs: BTreeMap<String, InputDef>,
    #[serde(default)]
    pub locals: BTreeMap<String, Value>,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub outputs: BTreeMap<String, Value>,
}

impl Workflow {
    /// Names of all workflows referenced by any step.
    pub fn referenced_workflows(&self) -> impl Iterator<Item = &str> {
        self.steps.iter().flat_map(Step::referenced_workflows)
    }
}
