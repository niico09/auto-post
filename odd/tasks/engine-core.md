# Feature: engine-core (auto-post)

## Objective
Rust workspace: a reusable engine crate (`auto-post-core`) plus a CLI (`auto-post-cli`) that runs JSON-manifest HTTP workflows with chaining, scoped variables and automatic recovery (e.g. re-login on expired JWT). UI comes later on top of the same crate.

## Design (accepted)
- `requests/*.json`: reusable HTTP definitions with a declared contract (`inputs`, `outputs`). They never reference globals or other steps directly.
- `workflows/*.json`: ordered steps. Each step binds a request (or a nested workflow) and maps its inputs from any scope. Bindings live in the workflow, so one request used in two workflows never pollutes either contract.
- Variable scopes: `globals` (project-wide, `globals.json` + env override), `locals` (per workflow run), `steps.<id>.*` (outputs of earlier steps), `inputs.*` (request contract).
- Templating: `{{scope.path}}`. Extraction: JSONPath-like on response body/headers/status.
- Recovery: a step declares `on_status: { "401": { "run": "<workflow>", "then": "retry" , "max": 1 } }`. The recovery workflow may update globals (e.g. new JWT); the failed step is retried and execution continues from it.
- Nested workflows: a step can call another workflow with explicit input mapping and receive its declared outputs.

## Tech
Rust workspace, crates: `auto-post-core` (lib), `auto-post-cli` (bin). reqwest + tokio, serde/serde_json, thiserror, clap. HTTP behind a trait for testability.

## Config
- TDD: not enabled (no explicit choice). Source: none. Runner: `cargo test`. Ordinary functional checks apply.
- Delivery: ask-on-risk. Planning heuristic ~400 authored lines per task (advisory).

## Tasks
- [ ] T1 Workspace skeleton + manifest model (serde types, loading, validation errors)
- [ ] T2 Variable scopes + template resolver
- [ ] T3 HTTP executor behind `HttpClient` trait + response extraction
- [ ] T4 Workflow runner: sequential steps, chaining, nested workflows
- [ ] T5 Recovery/retry (on_status -> run workflow -> retry step)
- [ ] T6 CLI (`run`, `validate`) + example manifests + README

## Route declaration
Delegated direct: one writer per task group (2+ non-trivial files, writer trigger).

## Progress / evidence
_(none yet)_

## Next step
T1
