//! A validated collection of requests, workflows and initial globals.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::model::{RequestDef, Workflow};
use super::validation::{validate, ValidationError};

/// Immutable, validated manifest set. Can only be built through
/// [`Project::from_parts`] (or the loader), so every instance is consistent.
#[derive(Debug, Clone)]
pub struct Project {
    requests: BTreeMap<String, RequestDef>,
    workflows: BTreeMap<String, Workflow>,
    globals: Map<String, Value>,
}

impl Project {
    /// Builds a project, validating cross references and workflow cycles.
    pub fn from_parts(
        requests: BTreeMap<String, RequestDef>,
        workflows: BTreeMap<String, Workflow>,
        globals: Map<String, Value>,
    ) -> Result<Self, ValidationError> {
        validate(&requests, &workflows)?;
        Ok(Self {
            requests,
            workflows,
            globals,
        })
    }

    pub fn request(&self, name: &str) -> Option<&RequestDef> {
        self.requests.get(name)
    }

    pub fn workflow(&self, name: &str) -> Option<&Workflow> {
        self.workflows.get(name)
    }

    pub fn request_names(&self) -> impl Iterator<Item = &str> {
        self.requests.keys().map(String::as_str)
    }

    pub fn workflow_names(&self) -> impl Iterator<Item = &str> {
        self.workflows.keys().map(String::as_str)
    }

    /// A copy of the globals declared in `globals.json`, to seed a run.
    pub fn initial_globals(&self) -> Map<String, Value> {
        self.globals.clone()
    }
}
