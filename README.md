# auto-post

A small engine and CLI that runs **HTTP workflows described in JSON**: chain
requests, pass values between them, and recover automatically when something
fails (for example, re-login when a JWT expires and retry).

The engine (`auto-post-core`) is a library; the CLI (`auto-post-cli`, binary
`auto-post`) is a thin layer on top. A UI is planned on the same crate.

## Concept

A project is a directory:

```
my-project/
  globals.json        optional, project-wide variables
  requests/*.json     reusable HTTP definitions
  workflows/*.json    ordered steps that bind requests together
```

The file name (without `.json`) is the request/workflow name.

**Request definitions vs. workflow bindings.** A request declares what it needs
(`inputs`) and what it produces (`outputs`). It never mentions globals or other
steps: templates inside a request can only read `{{inputs.*}}`. A workflow step
*binds* the request's inputs from whatever scope it wants. The same request can
therefore be used in two workflows without either contract leaking into the
other.

**Scopes** (readable in workflow templates as `{{scope.path}}`):

| Scope | Content | Lifetime |
| --- | --- | --- |
| `globals` | `globals.json`, `--global` overrides, values written by `set_global` | whole execution, shared and mutable |
| `locals` | the workflow's `locals` | one workflow run |
| `inputs` | the workflow's declared inputs | one workflow run |
| `steps.<id>` | outputs of earlier steps of this workflow | one workflow run |

Inside a request only `inputs` is available.

**Templates.** `"{{steps.login.token}}"` as the whole string keeps the JSON type
of the value (number, object, ...). Embedded in longer text
(`"Bearer {{inputs.token}}"`) values are stringified. A missing variable is an
error naming the path.

**Recovery.** A request step can declare `on_status`. When the response has that
status, the recovery workflow runs (it may update globals, e.g. store a new
token) and the failed step is retried with its bindings re-resolved, up to
`max` times. Any other non-2xx status without a policy fails the run.

## Manifest reference

Unknown fields are rejected everywhere.

### `globals.json`

A JSON object: `{ "base_url": "https://api.example.com", "user": "demo" }`.

### `requests/<name>.json`

```json
{
  "method": "POST",
  "url": "{{inputs.base_url}}/auth/login",
  "headers": { "Content-Type": "application/json" },
  "body": { "username": "{{inputs.username}}" },
  "inputs": { "base_url": {}, "username": {}, "page": { "default": 1 } },
  "outputs": { "token": { "from": "body", "path": "$.token" } }
}
```

* `method`: `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `HEAD`. `url` is required.
* `headers` (string values) and `body` (any JSON, sent as JSON) are optional and
  may contain templates over `inputs`.
* `inputs`: an input without `default` is required.
* `outputs`: extractors, see below.

### Extractors

`{ "from": "body" | "header" | "status", "path": ... }`

* `body`: `path` is `$.a.b[0].c`; omitted means the whole parsed JSON body.
* `header`: `path` is the header name (case-insensitive); the value is a string.
* `status`: the numeric status code; `path` is ignored.

### `workflows/<name>.json`

```json
{
  "inputs": { "user": { "default": "demo" } },
  "locals": { "api": "{{globals.base_url}}" },
  "steps": [ ... ],
  "outputs": { "name": "{{steps.me.name}}" }
}
```

* `inputs`: same shape as request inputs. `locals`: values resolved at start over
  `globals` and `inputs`. `outputs`: templates resolved after the last step;
  they are what the CLI prints and what a calling step receives.

Each step has an `id` (unique in the workflow) and exactly one of `request` or
`workflow`:

```json
{
  "id": "me",
  "request": "get_profile",
  "with": { "base_url": "{{locals.api}}", "token": "{{globals.token}}" },
  "extract": { "status": { "from": "status" } },
  "set_global": { "token": "{{steps.me.token}}" },
  "on_status": { "401": { "run": "login", "then": "retry", "max": 1 } }
}
```

* `with`: input bindings (all required inputs must be bound; unknown ones are
  rejected). Values may be templates over `globals`, `locals`, `steps`, `inputs`.
* `workflow`: instead of `request`, calls a nested workflow; `with` maps its
  inputs and the step's outputs are that workflow's `outputs`. `extract` and
  `on_status` are not allowed on workflow steps. Workflow cycles are rejected.
* `extract`: extra outputs (request steps), overriding declared outputs with the
  same name.
* `set_global`: after the step succeeds, each value is resolved (the step's own
  outputs are visible as `steps.<id>.*`) and stored in `globals`.
* `on_status`: keyed by HTTP status. `run` is a workflow name that must not have
  required inputs; `then` is `retry` (the only action, default); `max` defaults
  to 1. Each status counts its own attempts; exceeding `max` fails the run.

## CLI

```
auto-post validate [--dir <project>]
auto-post run <workflow> [--dir <project>] [--input KEY=VALUE]...
                         [--global KEY=VALUE]... [--verbose]
```

* `--dir` defaults to the current directory.
* `validate` prints `OK: ...` or the first error (unknown references, missing
  bindings, duplicate step ids, cycles, malformed JSON with the file path).
* `run` prints the workflow outputs as pretty JSON on stdout.
* `--input` / `--global`: `VALUE` is parsed as JSON when valid (`3`, `true`,
  `{"a":1}`), otherwise used as a string. `--global` overrides `globals.json`
  (handy for secrets or per-environment URLs).
* `--verbose` logs each request and status on stderr. Values of `Authorization`,
  `Cookie` (and their proxy/set variants) headers are masked as `***`. URLs are
  logged as written, so avoid secrets in query strings.
* HTTP requests time out after 30 seconds.
* Exit codes: `0` success, `1` validation or run failure, `2` usage error.

Try the example (the base URL is a placeholder, so `run` needs a real API; the
test suite runs it against a fake server):

```
cargo run -p auto-post-cli -- validate --dir examples/basic
cargo run -p auto-post-cli -- run profile --dir examples/basic \
    --global base_url=https://your-api.test --global password=secret --verbose
```

`examples/basic` shows globals, locals, a nested workflow (`profile` calls
`login`), `set_global` to share the login token, and a 401 recovery that re-runs
`login` and retries `get_profile`.

## Development

```
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The runner depends on the `HttpClient` trait, so workflows are tested with fake
transports and no network.

## Roadmap

* A UI on top of `auto-post-core` to edit manifests and inspect runs.
* Structured run events (per step, per recovery) instead of request-level
  logging.
* More recovery actions beyond `retry`, and non-JSON bodies.
