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
- [x] T1 Workspace skeleton + manifest model (serde types, loading, validation errors)
- [x] T2 Variable scopes + template resolver
- [x] T3 HTTP executor behind `HttpClient` trait + response extraction
- [x] T4 Workflow runner: sequential steps, chaining, nested workflows
- [x] T5 Recovery/retry (on_status -> run workflow -> retry step)
- [x] T6 CLI (`run`, `validate`) + example manifests + README

## Route declaration
Delegated direct: one writer per task group (2+ non-trivial files, writer trigger).

## Progress / evidence
- T1 bdfaf73, T2 7a747cc, T3 b877a18, T4 fe9dfe6, T5 129696a (branch feat/engine-core).
- Checks: cargo fmt --all --check ok; cargo clippy --workspace --all-targets -- -D warnings ok; cargo test --workspace 42 passed.
- T6: 78709ed (core: default 30s timeout), 43bcae6 (cli + examples/basic + tests), README commit (docs). Checks: fmt ok; clippy -D warnings ok; cargo test --workspace 44 core + 14 cli passed; `validate --dir examples/basic` -> OK: 3 request(s), 2 workflow(s).
- Step model additions: `set_global` (map name -> template, applied after step success), step-level `extract`, workflow `inputs`/`outputs`. Non-2xx without matching `on_status` => UnexpectedStatus error.

## Next step
Feature complete on branch feat/engine-core; user decides push/PR (delivery: ask-on-risk).
