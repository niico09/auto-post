//! Workflow execution.
//!
//! Each workflow run owns its `inputs`, `locals` and `steps` scopes (isolated
//! from other runs, including nested ones); `globals` are shared and mutable
//! across the whole execution.

mod error;
mod request;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use serde_json::{Map, Value};

use crate::extract::extract;
use crate::http::{HttpClient, HttpResponse};
use crate::manifest::{Extractor, Project, RequestDef, Step, StepTarget};
use crate::template::{resolve_map, Scopes};

pub use error::RunError;
use request::{bind_inputs, build_request};

type JsonMap = Map<String, Value>;
type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Executes workflows of a [`Project`] through an injected [`HttpClient`].
pub struct Runner<'a> {
    project: &'a Project,
    http: &'a dyn HttpClient,
}

/// State private to one workflow run.
struct WorkflowRun {
    inputs: JsonMap,
    locals: JsonMap,
    steps: JsonMap,
}

impl WorkflowRun {
    fn scopes<'s>(&'s self, globals: &'s JsonMap) -> Scopes<'s> {
        Scopes::new()
            .with("globals", globals)
            .with("locals", &self.locals)
            .with("steps", &self.steps)
            .with("inputs", &self.inputs)
    }
}

impl<'a> Runner<'a> {
    pub fn new(project: &'a Project, http: &'a dyn HttpClient) -> Self {
        Self { project, http }
    }

    /// Runs `workflow` with the given inputs. `globals` are read and may be
    /// updated by `set_global` steps; the workflow's declared outputs are
    /// returned.
    pub async fn run(
        &self,
        workflow: &str,
        inputs: JsonMap,
        globals: &mut JsonMap,
    ) -> Result<JsonMap, RunError> {
        self.run_workflow(workflow, inputs, globals).await
    }

    fn run_workflow<'s>(
        &'s self,
        name: &'s str,
        provided: JsonMap,
        globals: &'s mut JsonMap,
    ) -> BoxFuture<'s, Result<JsonMap, RunError>> {
        Box::pin(async move {
            let workflow = self
                .project
                .workflow(name)
                .ok_or_else(|| RunError::UnknownWorkflow(name.to_owned()))?;
            let inputs = bind_inputs(name, &workflow.inputs, provided)?;
            let locals = resolve_map(
                &workflow.locals,
                &Scopes::new()
                    .with("globals", globals)
                    .with("inputs", &inputs),
            )?;
            let mut run = WorkflowRun {
                inputs,
                locals,
                steps: Map::new(),
            };
            for step in &workflow.steps {
                self.execute_step(&mut run, step, globals)
                    .await
                    .map_err(|source| RunError::InStep {
                        workflow: name.to_owned(),
                        step: step.id.clone(),
                        source: Box::new(source),
                    })?;
            }
            Ok(resolve_map(&workflow.outputs, &run.scopes(globals))?)
        })
    }

    async fn execute_step(
        &self,
        run: &mut WorkflowRun,
        step: &Step,
        globals: &mut JsonMap,
    ) -> Result<(), RunError> {
        let outputs = match &step.target {
            StepTarget::Request(name) => self.run_request_step(run, step, name, globals).await?,
            StepTarget::Workflow(name) => {
                let with = resolve_map(&step.with, &run.scopes(globals))?;
                self.run_workflow(name, with, globals).await?
            }
        };
        run.steps.insert(step.id.clone(), Value::Object(outputs));
        let updates = resolve_map(&step.set_global, &run.scopes(globals))?;
        globals.extend(updates);
        Ok(())
    }

    async fn run_request_step(
        &self,
        run: &WorkflowRun,
        step: &Step,
        request_name: &str,
        globals: &mut JsonMap,
    ) -> Result<JsonMap, RunError> {
        let def = self
            .project
            .request(request_name)
            .ok_or_else(|| RunError::UnknownRequest(request_name.to_owned()))?;
        // Bindings are re-resolved on every attempt so a retry sees globals
        // changed by recovery.
        let with = resolve_map(&step.with, &run.scopes(globals))?;
        let inputs = bind_inputs(request_name, &def.inputs, with)?;
        let response = self.http.send(build_request(def, &inputs)?).await?;
        if !(200..300).contains(&response.status()) {
            return Err(RunError::UnexpectedStatus {
                request: request_name.to_owned(),
                status: response.status(),
            });
        }
        extract_outputs(def, step, &response)
    }
}

/// Request-declared outputs, overridden by step-level `extract` entries.
fn extract_outputs(
    def: &RequestDef,
    step: &Step,
    response: &HttpResponse,
) -> Result<JsonMap, RunError> {
    let extractors: BTreeMap<&String, &Extractor> =
        def.outputs.iter().chain(step.extract.iter()).collect();
    let mut outputs = Map::new();
    for (name, extractor) in extractors {
        let value = extract(response, extractor).map_err(|source| RunError::Extract {
            output: name.clone(),
            source,
        })?;
        outputs.insert(name.clone(), value);
    }
    Ok(outputs)
}
