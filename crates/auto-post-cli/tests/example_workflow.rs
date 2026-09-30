//! End-to-end run of `examples/basic` against a fake HTTP server (no network).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use auto_post_cli::args::{parse_key_value, RunArgs};
use auto_post_cli::commands;
use auto_post_cli::logging::LoggingClient;
use auto_post_core::http::{HttpClient, HttpError, HttpMethod, HttpRequest, HttpResponse};
use serde_json::{json, Value};

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic")
}

/// Fake API: `/auth/login` issues `fresh`, `/auth/refresh` issues `rotated`,
/// and the first `/me` call is rejected with 401 (expired token) to trigger
/// the login recovery.
#[derive(Default)]
struct FakeApi {
    requests: Mutex<Vec<HttpRequest>>,
    me_calls: Mutex<u32>,
}

impl FakeApi {
    fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|r| format!("{:?} {}", r.method, r.url))
            .collect()
    }
}

fn reply(status: u16, body: Value) -> Result<HttpResponse, HttpError> {
    Ok(HttpResponse::new(status, [], body.to_string()))
}

#[async_trait]
impl HttpClient for FakeApi {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, HttpError> {
        self.requests.lock().unwrap().push(request.clone());
        if request.url.ends_with("/auth/login") {
            assert_eq!(request.method, HttpMethod::Post);
            assert_eq!(
                request.body,
                Some(json!({"username": "demo", "password": "change-me"}))
            );
            return reply(200, json!({"token": "fresh"}));
        }
        if request.url.ends_with("/auth/refresh") {
            return reply(200, json!({"token": "rotated"}));
        }
        let mut calls = self.me_calls.lock().unwrap();
        *calls += 1;
        let authorized =
            request.headers.get("Authorization").map(String::as_str) == Some("Bearer fresh");
        if *calls == 1 || !authorized {
            return reply(401, json!({"error": "expired"}));
        }
        reply(200, json!({"name": "Ada", "email": "ada@example.com"}))
    }
}

fn run_args(globals: &[&str]) -> RunArgs {
    RunArgs {
        workflow: "profile".into(),
        dir: example_dir(),
        inputs: vec![],
        globals: globals
            .iter()
            .map(|g| parse_key_value(g).unwrap())
            .collect(),
        verbose: false,
    }
}

#[tokio::test]
async fn profile_workflow_recovers_from_401_and_prints_outputs() {
    let api = FakeApi::default();
    let mut out = Vec::new();
    commands::run(&run_args(&[]), &api, &mut out).await.unwrap();

    let printed: Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(
        printed,
        json!({"name": "Ada", "email": "ada@example.com", "token": "rotated"})
    );
    // nested login, /me -> 401, recovery login, /me retried, refresh
    assert_eq!(
        api.paths(),
        [
            "Post https://api.example.com/auth/login",
            "Get https://api.example.com/me",
            "Post https://api.example.com/auth/login",
            "Get https://api.example.com/me",
            "Post https://api.example.com/auth/refresh",
        ]
    );
}

#[tokio::test]
async fn global_override_replaces_globals_json_values() {
    let api = FakeApi::default();
    let mut out = Vec::new();
    commands::run(
        &run_args(&["base_url=https://staging.test"]),
        &api,
        &mut out,
    )
    .await
    .unwrap();
    assert!(api
        .paths()
        .iter()
        .all(|line| line.contains("https://staging.test/")));
}

#[tokio::test]
async fn verbose_logging_shows_recovery_without_leaking_tokens() {
    let lines = Mutex::new(Vec::<String>::new());
    let logging = LoggingClient::new(FakeApi::default(), |line: &str| {
        lines.lock().unwrap().push(line.to_owned());
    });
    let mut out = Vec::new();
    commands::run(&run_args(&[]), &logging, &mut out)
        .await
        .unwrap();

    let log = lines.into_inner().unwrap().join("\n");
    assert!(log.contains("<- 401 GET https://api.example.com/me"));
    assert!(log.contains("Authorization=***"));
    assert!(!log.contains("Bearer") && !log.contains("rotated"), "{log}");
}
