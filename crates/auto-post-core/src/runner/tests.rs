//! Runner tests using a scripted fake [`HttpClient`] (no network).

use std::sync::Mutex;

use async_trait::async_trait;
use serde_json::{json, Map, Value};

use super::*;
use crate::http::{HttpError, HttpRequest, HttpResponse};

type Handler = Box<dyn FnMut(&HttpRequest) -> HttpResponse + Send>;

/// Fake transport: answers with a closure and records every request.
pub(super) struct FakeHttp {
    handler: Mutex<Handler>,
    requests: Mutex<Vec<HttpRequest>>,
}

impl FakeHttp {
    pub(super) fn new(handler: impl FnMut(&HttpRequest) -> HttpResponse + Send + 'static) -> Self {
        Self {
            handler: Mutex::new(Box::new(handler)),
            requests: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn requests(&self) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().clone()
    }
}

#[async_trait]
impl HttpClient for FakeHttp {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        let response = (self.handler.lock().unwrap())(&request);
        self.requests.lock().unwrap().push(request);
        Ok(response)
    }
}

pub(super) fn reply(status: u16, body: Value) -> HttpResponse {
    HttpResponse::new(status, [], body.to_string())
}

pub(super) fn project(requests: Value, workflows: Value) -> Project {
    Project::from_parts(
        serde_json::from_value(requests).unwrap(),
        serde_json::from_value(workflows).unwrap(),
        Map::new(),
    )
    .unwrap()
}

pub(super) fn object(value: Value) -> Map<String, Value> {
    value.as_object().unwrap().clone()
}

fn sample_requests() -> Value {
    json!({
        "create_user": {
            "method": "POST", "url": "http://api/users",
            "body": {"name": "{{inputs.name}}"},
            "inputs": {"name": {}},
            "outputs": {"id": {"from": "body", "path": "$.id"}}
        },
        "get_user": {
            "method": "GET", "url": "http://api/users/{{inputs.id}}",
            "inputs": {"id": {}},
            "outputs": {"name": {"from": "body", "path": "$.name"}}
        }
    })
}

fn user_server() -> FakeHttp {
    FakeHttp::new(|request| {
        if request.url.ends_with("/users") {
            reply(201, json!({"id": 42}))
        } else {
            reply(200, json!({"name": "ada"}))
        }
    })
}

#[tokio::test]
async fn chains_steps_through_step_outputs_and_keeps_types() {
    let project = project(
        sample_requests(),
        json!({"main": {
            "inputs": {"name": {}},
            "steps": [
                {"id": "create", "request": "create_user", "with": {"name": "{{inputs.name}}"}},
                {"id": "fetch", "request": "get_user", "with": {"id": "{{steps.create.id}}"}}
            ],
            "outputs": {"user_id": "{{steps.create.id}}", "name": "{{steps.fetch.name}}"}
        }}),
    );
    let http = user_server();
    let outputs = Runner::new(&project, &http)
        .run("main", object(json!({"name": "ada"})), &mut Map::new())
        .await
        .unwrap();

    assert_eq!(outputs, object(json!({"user_id": 42, "name": "ada"})));
    let requests = http.requests();
    assert_eq!(requests[0].body, Some(json!({"name": "ada"})));
    assert_eq!(requests[1].url, "http://api/users/42");
}

#[tokio::test]
async fn locals_globals_and_step_extract_are_available_to_bindings() {
    let project = project(
        json!({"echo": {
            "method": "POST", "url": "http://api/echo",
            "body": {"v": "{{inputs.v}}"},
            "inputs": {"v": {}},
            "outputs": {"echoed": {"from": "body", "path": "$.v"}}
        }}),
        json!({"main": {
            "locals": {"prefix": "{{globals.env}}-x"},
            "steps": [
                {"id": "a", "request": "echo", "with": {"v": "{{locals.prefix}}"},
                 "extract": {"status": {"from": "status"}}},
                {"id": "b", "request": "echo", "with": {"v": "{{steps.a.status}}"}}
            ],
            "outputs": {"first": "{{steps.a.echoed}}", "second": "{{steps.b.echoed}}"}
        }}),
    );
    let http = FakeHttp::new(|request| reply(200, request.body.clone().unwrap()));
    let mut globals = object(json!({"env": "dev"}));
    let outputs = Runner::new(&project, &http)
        .run("main", Map::new(), &mut globals)
        .await
        .unwrap();
    assert_eq!(outputs, object(json!({"first": "dev-x", "second": 200})));
}

#[tokio::test]
async fn requests_cannot_read_globals_directly() {
    let project = project(
        json!({"leaky": {"method": "GET", "url": "http://api/{{globals.secret}}"}}),
        json!({"main": {"steps": [{"id": "s", "request": "leaky"}]}}),
    );
    let http = FakeHttp::new(|_| reply(200, json!({})));
    let err = Runner::new(&project, &http)
        .run("main", Map::new(), &mut object(json!({"secret": "x"})))
        .await
        .unwrap_err();
    assert!(matches!(
        err.root_cause(),
        RunError::Template(crate::template::TemplateError::UnknownScope { .. })
    ));
    assert!(http.requests().is_empty());
}

#[tokio::test]
async fn nested_workflow_gets_explicit_inputs_and_returns_declared_outputs_only() {
    let project = project(
        sample_requests(),
        json!({
            "make_user": {
                "inputs": {"who": {}},
                "steps": [{"id": "c", "request": "create_user", "with": {"name": "{{inputs.who}}"}}],
                "outputs": {"id": "{{steps.c.id}}"}
            },
            "main": {"steps": [
                {"id": "sub", "workflow": "make_user", "with": {"who": "grace"}},
                {"id": "fetch", "request": "get_user", "with": {"id": "{{steps.sub.id}}"}}
            ], "outputs": {"name": "{{steps.fetch.name}}"}}
        }),
    );
    let http = user_server();
    let outputs = Runner::new(&project, &http)
        .run("main", Map::new(), &mut Map::new())
        .await
        .unwrap();
    assert_eq!(outputs, object(json!({"name": "ada"})));
    assert_eq!(http.requests()[0].body, Some(json!({"name": "grace"})));
}

#[tokio::test]
async fn nested_runs_have_isolated_locals_and_steps_but_share_globals() {
    let project = project(
        sample_requests(),
        json!({
            "child": {
                "locals": {"mine": "child-local"},
                "steps": [
                    {"id": "c", "request": "create_user", "with": {"name": "{{locals.mine}}"},
                     "set_global": {"seen": "{{steps.c.id}}"}}
                ]
            },
            "main": {
                "locals": {"mine": "parent-local"},
                "steps": [
                    {"id": "sub", "workflow": "child"},
                    {"id": "leak", "request": "create_user", "with": {"name": "{{steps.c.id}}"}}
                ]
            }
        }),
    );
    let http = user_server();
    let mut globals = Map::new();
    let err = Runner::new(&project, &http)
        .run("main", Map::new(), &mut globals)
        .await
        .unwrap_err();
    // The child's step `c` is not visible in the parent.
    assert!(matches!(
        err.root_cause(),
        RunError::Template(crate::template::TemplateError::Missing { path }) if path == "steps.c.id"
    ));
    // ...but the global set by the child is shared.
    assert_eq!(globals.get("seen"), Some(&json!(42)));
    assert_eq!(
        http.requests()[0].body,
        Some(json!({"name": "child-local"}))
    );
}

#[tokio::test]
async fn same_request_in_two_workflows_with_different_bindings_stays_independent() {
    let project = project(
        json!({"echo": {
            "method": "POST", "url": "http://api/echo/{{inputs.kind}}",
            "body": {"v": "{{inputs.v}}"},
            "inputs": {"v": {}, "kind": {"default": "plain"}},
            "outputs": {"v": {"from": "body", "path": "$.v"}}
        }}),
        json!({
            "wf_a": {"steps": [{"id": "e", "request": "echo", "with": {"v": "alpha"}}],
                     "outputs": {"out": "{{steps.e.v}}"}},
            "wf_b": {"steps": [{"id": "e", "request": "echo", "with": {"v": 2, "kind": "special"}}],
                     "outputs": {"out": "{{steps.e.v}}"}}
        }),
    );
    let http = FakeHttp::new(|request| reply(200, request.body.clone().unwrap()));
    let runner = Runner::new(&project, &http);
    let out_a = runner
        .run("wf_a", Map::new(), &mut Map::new())
        .await
        .unwrap();
    let out_b = runner
        .run("wf_b", Map::new(), &mut Map::new())
        .await
        .unwrap();
    let out_a_again = runner
        .run("wf_a", Map::new(), &mut Map::new())
        .await
        .unwrap();

    assert_eq!(out_a, object(json!({"out": "alpha"})));
    assert_eq!(out_b, object(json!({"out": 2})));
    assert_eq!(out_a_again, out_a);
    let requests = http.requests();
    assert_eq!(requests[0].url, "http://api/echo/plain");
    assert_eq!(requests[0].body, Some(json!({"v": "alpha"})));
    assert_eq!(requests[1].url, "http://api/echo/special");
    assert_eq!(requests[1].body, Some(json!({"v": 2})));
}

#[tokio::test]
async fn top_level_input_contract_is_enforced() {
    let project = project(
        sample_requests(),
        json!({"main": {"inputs": {"name": {}}, "steps": []}}),
    );
    let http = user_server();
    let runner = Runner::new(&project, &http);
    let missing = runner.run("main", Map::new(), &mut Map::new()).await;
    assert!(matches!(missing, Err(RunError::MissingInput { input, .. }) if input == "name"));
    let unknown = runner
        .run(
            "main",
            object(json!({"name": "a", "zzz": 1})),
            &mut Map::new(),
        )
        .await;
    assert!(matches!(unknown, Err(RunError::UnknownInput { input, .. }) if input == "zzz"));
    let ghost = runner.run("ghost", Map::new(), &mut Map::new()).await;
    assert!(matches!(ghost, Err(RunError::UnknownWorkflow(_))));
}

#[tokio::test]
async fn unexpected_status_without_recovery_fails_with_step_context() {
    let project = project(
        sample_requests(),
        json!({"main": {"steps": [{"id": "s", "request": "get_user", "with": {"id": 1}}]}}),
    );
    let http = FakeHttp::new(|_| reply(500, json!({})));
    let err = Runner::new(&project, &http)
        .run("main", Map::new(), &mut Map::new())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("workflow `main`, step `s`"));
    assert!(matches!(
        err.root_cause(),
        RunError::UnexpectedStatus { status: 500, .. }
    ));
}
