//! Cross-reference validation of requests and workflows.

use std::collections::{BTreeMap, HashSet};

use serde_json::Value;
use thiserror::Error;

use super::model::{InputDef, RequestDef, StepTarget, Workflow};

/// A manifest inconsistency, always naming the offending workflow/step.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("workflow `{workflow}`, step `{step}`: unknown request `{request}`")]
    UnknownRequest {
        workflow: String,
        step: String,
        request: String,
    },
    #[error("workflow `{workflow}`, step `{step}`: unknown workflow `{target}`")]
    UnknownWorkflow {
        workflow: String,
        step: String,
        target: String,
    },
    #[error("workflow `{workflow}`: duplicate step id `{step}`")]
    DuplicateStepId { workflow: String, step: String },
    #[error(
        "workflow `{workflow}`, step `{step}`: missing required input `{input}` of `{target}`"
    )]
    MissingInputBinding {
        workflow: String,
        step: String,
        target: String,
        input: String,
    },
    #[error("workflow `{workflow}`, step `{step}`: `{target}` declares no input `{input}`")]
    UnknownInputBinding {
        workflow: String,
        step: String,
        target: String,
        input: String,
    },
    #[error(
        "workflow `{workflow}`, step `{step}`: recovery workflow `{target}` has required input `{input}` but recovery runs without inputs"
    )]
    RecoveryNeedsInput {
        workflow: String,
        step: String,
        target: String,
        input: String,
    },
    #[error("workflow cycle detected: {}", .cycle.join(" -> "))]
    WorkflowCycle { cycle: Vec<String> },
}

pub(super) fn validate(
    requests: &BTreeMap<String, RequestDef>,
    workflows: &BTreeMap<String, Workflow>,
) -> Result<(), ValidationError> {
    for (name, workflow) in workflows {
        validate_workflow(name, workflow, requests, workflows)?;
    }
    detect_cycles(workflows)
}

fn validate_workflow(
    name: &str,
    workflow: &Workflow,
    requests: &BTreeMap<String, RequestDef>,
    workflows: &BTreeMap<String, Workflow>,
) -> Result<(), ValidationError> {
    let mut seen = HashSet::new();
    for step in &workflow.steps {
        if !seen.insert(step.id.as_str()) {
            return Err(ValidationError::DuplicateStepId {
                workflow: name.to_owned(),
                step: step.id.clone(),
            });
        }
        let (target, declared) = match &step.target {
            StepTarget::Request(request) => {
                let def = requests
                    .get(request)
                    .ok_or_else(|| ValidationError::UnknownRequest {
                        workflow: name.to_owned(),
                        step: step.id.clone(),
                        request: request.clone(),
                    })?;
                (request, &def.inputs)
            }
            StepTarget::Workflow(target) => {
                let callee =
                    workflows
                        .get(target)
                        .ok_or_else(|| ValidationError::UnknownWorkflow {
                            workflow: name.to_owned(),
                            step: step.id.clone(),
                            target: target.clone(),
                        })?;
                (target, &callee.inputs)
            }
        };
        check_bindings(name, &step.id, target, declared, &step.with)?;

        for policy in step.on_status.values() {
            let recovery =
                workflows
                    .get(&policy.run)
                    .ok_or_else(|| ValidationError::UnknownWorkflow {
                        workflow: name.to_owned(),
                        step: step.id.clone(),
                        target: policy.run.clone(),
                    })?;
            if let Some((input, _)) = recovery.inputs.iter().find(|(_, def)| def.is_required()) {
                return Err(ValidationError::RecoveryNeedsInput {
                    workflow: name.to_owned(),
                    step: step.id.clone(),
                    target: policy.run.clone(),
                    input: input.clone(),
                });
            }
        }
    }
    Ok(())
}

fn check_bindings(
    workflow: &str,
    step: &str,
    target: &str,
    declared: &BTreeMap<String, InputDef>,
    with: &BTreeMap<String, Value>,
) -> Result<(), ValidationError> {
    if let Some(input) = with.keys().find(|key| !declared.contains_key(*key)) {
        return Err(ValidationError::UnknownInputBinding {
            workflow: workflow.to_owned(),
            step: step.to_owned(),
            target: target.to_owned(),
            input: input.clone(),
        });
    }
    let missing = declared
        .iter()
        .find(|(key, def)| def.is_required() && !with.contains_key(*key));
    match missing {
        Some((input, _)) => Err(ValidationError::MissingInputBinding {
            workflow: workflow.to_owned(),
            step: step.to_owned(),
            target: target.to_owned(),
            input: input.clone(),
        }),
        None => Ok(()),
    }
}

fn detect_cycles(workflows: &BTreeMap<String, Workflow>) -> Result<(), ValidationError> {
    let mut done = HashSet::new();
    let mut stack = Vec::new();
    for name in workflows.keys() {
        visit(name, workflows, &mut done, &mut stack)?;
    }
    Ok(())
}

fn visit<'a>(
    name: &'a str,
    workflows: &'a BTreeMap<String, Workflow>,
    done: &mut HashSet<&'a str>,
    stack: &mut Vec<&'a str>,
) -> Result<(), ValidationError> {
    if let Some(position) = stack.iter().position(|visiting| *visiting == name) {
        let mut cycle: Vec<String> = stack[position..].iter().map(|n| (*n).to_owned()).collect();
        cycle.push(name.to_owned());
        return Err(ValidationError::WorkflowCycle { cycle });
    }
    if done.contains(name) {
        return Ok(());
    }
    stack.push(name);
    if let Some(workflow) = workflows.get(name) {
        for dependency in workflow.referenced_workflows() {
            visit(dependency, workflows, done, stack)?;
        }
    }
    stack.pop();
    done.insert(name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Project;
    use serde_json::{json, Map};

    fn project(requests: Value, workflows: Value) -> Result<Project, ValidationError> {
        Project::from_parts(
            serde_json::from_value(requests).unwrap(),
            serde_json::from_value(workflows).unwrap(),
            Map::new(),
        )
    }

    fn requests() -> Value {
        json!({
            "echo": {"method": "POST", "url": "http://x/echo", "inputs": {"v": {}, "opt": {"default": 1}}},
            "ping": {"method": "GET", "url": "http://x/ping"}
        })
    }

    #[test]
    fn accepts_valid_project() {
        let workflows = json!({
            "main": {"steps": [
                {"id": "a", "request": "echo", "with": {"v": "1"}},
                {"id": "b", "request": "ping"}
            ]}
        });
        assert!(project(requests(), workflows).is_ok());
    }

    #[test]
    fn rejects_unknown_request() {
        let workflows = json!({"main": {"steps": [{"id": "a", "request": "nope"}]}});
        assert_eq!(
            project(requests(), workflows).unwrap_err(),
            ValidationError::UnknownRequest {
                workflow: "main".into(),
                step: "a".into(),
                request: "nope".into()
            }
        );
    }

    #[test]
    fn rejects_unknown_workflow_reference_and_recovery_target() {
        let call = json!({"main": {"steps": [{"id": "a", "workflow": "ghost"}]}});
        assert!(matches!(
            project(requests(), call).unwrap_err(),
            ValidationError::UnknownWorkflow { target, .. } if target == "ghost"
        ));
        let recovery = json!({"main": {"steps": [
            {"id": "a", "request": "ping", "on_status": {"401": {"run": "ghost"}}}
        ]}});
        assert!(matches!(
            project(requests(), recovery).unwrap_err(),
            ValidationError::UnknownWorkflow { target, .. } if target == "ghost"
        ));
    }

    #[test]
    fn rejects_duplicate_step_ids() {
        let workflows = json!({"main": {"steps": [
            {"id": "a", "request": "ping"}, {"id": "a", "request": "ping"}
        ]}});
        assert_eq!(
            project(requests(), workflows).unwrap_err(),
            ValidationError::DuplicateStepId {
                workflow: "main".into(),
                step: "a".into()
            }
        );
    }

    #[test]
    fn rejects_missing_and_unknown_bindings() {
        let missing = json!({"main": {"steps": [{"id": "a", "request": "echo"}]}});
        assert!(matches!(
            project(requests(), missing).unwrap_err(),
            ValidationError::MissingInputBinding { input, .. } if input == "v"
        ));
        let unknown = json!({"main": {"steps": [
            {"id": "a", "request": "echo", "with": {"v": 1, "zzz": 2}}
        ]}});
        assert!(matches!(
            project(requests(), unknown).unwrap_err(),
            ValidationError::UnknownInputBinding { input, .. } if input == "zzz"
        ));
    }

    #[test]
    fn nested_workflow_bindings_are_checked_against_its_inputs() {
        let workflows = json!({
            "child": {"inputs": {"who": {}}, "steps": [{"id": "p", "request": "ping"}]},
            "main": {"steps": [{"id": "c", "workflow": "child"}]}
        });
        assert!(matches!(
            project(requests(), workflows).unwrap_err(),
            ValidationError::MissingInputBinding { target, input, .. }
                if target == "child" && input == "who"
        ));
    }

    #[test]
    fn detects_direct_and_indirect_cycles() {
        let direct = json!({"a": {"steps": [{"id": "s", "workflow": "a"}]}});
        assert_eq!(
            project(requests(), direct).unwrap_err(),
            ValidationError::WorkflowCycle {
                cycle: vec!["a".into(), "a".into()]
            }
        );
        let indirect = json!({
            "a": {"steps": [{"id": "s", "workflow": "b"}]},
            "b": {"steps": [{"id": "s", "request": "ping", "on_status": {"401": {"run": "a"}}}]}
        });
        assert_eq!(
            project(requests(), indirect).unwrap_err(),
            ValidationError::WorkflowCycle {
                cycle: vec!["a".into(), "b".into(), "a".into()]
            }
        );
    }

    #[test]
    fn recovery_workflow_may_not_require_inputs() {
        let workflows = json!({
            "login": {"inputs": {"user": {}}, "steps": [{"id": "s", "request": "ping"}]},
            "main": {"steps": [
                {"id": "a", "request": "ping", "on_status": {"401": {"run": "login"}}}
            ]}
        });
        assert!(matches!(
            project(requests(), workflows).unwrap_err(),
            ValidationError::RecoveryNeedsInput { input, .. } if input == "user"
        ));
    }

    #[test]
    fn step_shape_is_enforced_at_deserialization() {
        for step in [
            json!({"id": "a"}),
            json!({"id": "a", "request": "x", "workflow": "y"}),
            json!({"id": "a", "workflow": "y", "extract": {"t": {"from": "status"}}}),
        ] {
            assert!(serde_json::from_value::<crate::manifest::Step>(step).is_err());
        }
    }
}
