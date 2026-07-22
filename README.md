# simple-pg-agent

Interactive PostgreSQL assistant that runs in your terminal. You describe what you
want in plain English; an LLM plans the work; the agent issues safe, parameterised
SQL through a fixed tool set.

- REPL-style assistant backed by any OpenAI-compatible chat model
- Tool-driven SQL generation through `sqlx` against PostgreSQL
- Seven introspecting and mutating DB tools, each guarded by allow-lists
- Parallel tool execution per turn, bounded by `max_iterations`
- Markdown-style terminal rendering via `termimad`

## Quick start

Prerequisites: a Rust toolchain, a reachable PostgreSQL instance, and credentials
for an OpenAI-compatible endpoint.

```sh
cp .env .env          # then fill in the values below
make run              # cargo run
```

Required environment (loaded by `dotenv` in `src/main.rs`):

- `OPENAI_BASE_URL` — OpenAI-compatible endpoint
- `OPENAI_API_KEY`  — API key
- `MODEL`           — chat model id
- `DATABASE_URL`    — PostgreSQL connection string

## Architecture

Each user turn is an agentic loop. The prompt is forwarded to the LLM together
with the registered tool list. The LLM either replies directly or emits one or
more tool calls. The agent executes those calls concurrently, appends the
results back into the conversation, and asks again — up to `AgentConfig::max_iterations`
(default `10`). When the model answers without tool calls, the reply is rendered
with `termimad` and the REPL returns to the prompt.

```
stdin  ┌────────────┐  messages + tools  ┌──────────┐
──────▶│  PGAgent   │───────────────────▶│  LLM     │
       │  (REPL)    │◀───────────────────│  client  │
       └─────┬──────┘  reply / tool calls└──────────┘
             │ tool calls (fan out with tokio::spawn)
             ▼
       ┌──────────────┐
       │ ToolRegistry │── sqlx :: PgPool ──▶ PostgreSQL
       └──────────────┘
```

`src/main.rs` wires the loop:

1. `LLMClient::from_env()` — build the chat client from env vars.
2. `db::establish_connection(DATABASE_URL)` — open a shared `PgPool`.
3. `register_db_tools(&mut ToolRegistry, pool)` — register every DB tool.
4. `PGAgent::new(tools, llm)` then `agent.run()` — drop into the rustyline REPL.

## Modules

### `agent` — `PGAgent`
The orchestrator (`src/agent.rs`).

- Owns `messages: Vec<ChatCompletionRequestMessage>`, an `Arc<ToolRegistry>`,
  the `LLMClient`, and an `AgentConfig`. Seeds `messages` with the system
  prompt via `context::init_system_prompts`.
- `run()` drives the rustyline REPL: prints `>> `, reads a line, appends it to
  history (`.history/history.txt`), and dispatches each line through
  `handle_input`. Handles `Ctrl-C` and `Ctrl-D` gracefully.
- `handle_input(msg)` runs the per-turn loop:
  1. Append the user message.
  2. Call `llm_cli.chat(messages, tools)`.
  3. If the model returned no tool calls, render `content` with `termimad` and stop.
  4. Otherwise, fan out every tool call concurrently with `tokio::spawn`,
     collect their `String` outputs, push one `assistant` tool_calls message
     plus one `tool` message per result, and loop.
- Returns `AgentError::ExceedMaxIter` if the model keeps calling tools past the
  iteration cap.

### `llm` — `LLMClient`
Thin wrapper over `async-openai` using the BYOT (Bring Your Own Types) escape
hatch (`src/llm.rs`).

- `LLMClient::from_env` reads `OPENAI_BASE_URL`, `OPENAI_API_KEY`, and `MODEL`
  and constructs an `OpenAIConfig` + `Client`.
- `chat(msgs, tools)` builds a `CreateChatCompletionRequest` (tokens cap
  `5000`), forwards `tools`, and deserialises the response into a custom
  `CompatibleChatCompletionResponse` so that provider-specific `service_tier`
  values (e.g. `"standard"`) — which `async-openai` does not know about — do
  not break parsing. Covered by a unit test in the same file.
- Also hosts small message-builder helpers (`user_message`, `tool_result`) and
  a `strip_think` utility for trimming leading `...` blocks from model output.

### `tool` — `Tool` / `ToolRegistry`
The extension point for capabilities (`src/tool.rs`).

- `Tool` is an `async_trait` exposing `name`, `description`,
  `parammeters_schema` (a JSON Schema `serde_json::Value`), and
  `async execute(args) -> Result<String, AgentError>`.
- `ToolRegistry` is a `HashMap<String, Arc<dyn Tool>>` with:
  - `register(tool)` to register by `name`,
  - `to_openai_funcs()` to flatten the registry into `Vec<ChatCompletionTools>`
    for the LLM (each tool becomes a strict-mode `ChatCompletionTool`),
  - `async call(name, args_json)` to parse JSON args and dispatch.

### `context`
`src/context.rs` simply `include_str!`s `docs/system_prompts.md` at compile
time and returns it wrapped as the opening `system` chat message.

### `db` — PostgreSQL toolkit
Built on `sqlx::PgPool`. `db/mod.rs` provides:

- `establish_connection(url)` — opens a pool (`max_connections = 50`,
  `acquire_timeout = 5s`, `idle_timeout = 10s`).
- `register_db_tools(registry, pool)` — registers every DB tool.
- Internal helpers reused by each tool:
  - `get_schema_table` — parses `schema.table` arguments, defaulting to `public`.
  - `is_safe_identifier` — guards quoted identifiers.
  - `split_statements` — splits on `;` to reject multi-statement scripts.
  - `require_leading_keyword(sql, ALLOWED)` — enforces a leading-keyword allow-list.
  - `shape_rows` — flattens `PgRow` results into `{rows, columns}` JSON the
    LLM can consume, falling back to `<unprintable: type>` for unreadable cells.
  - `extract_params` — reads optional `params` arrays for `sqlx::query::bind`.

The seven DB tools exposed to the LLM:

| Tool              | Purpose                                                                                          |
|-------------------|--------------------------------------------------------------------------------------------------|
| `list_schemas`    | All accessible schemas from `information_schema.schemata`.                                      |
| `list_tables`     | Tables (optionally views) inside a schema.                                                      |
| `desc_table`      | Columns, primary key, foreign keys, and indexes for a `schema.table`.                            |
| `search_schema`   | Fuzzy search over table/column names across every schema.                                       |
| `execute_query`   | `SELECT` / `WITH` / `EXPLAIN` (allow-listed) with bound params, truncated to `limit` (`max_rows`).|
| `execute_dml`     | `INSERT` / `UPDATE` / `DELETE` / `MERGE`, single statement; `preview: true` runs a same-shape `SELECT COUNT(*)` instead of mutating, so you can see blast radius first. |
| `execute_ddl`     | `CREATE` / `ALTER` / `DROP` / `TRUNCATE`, single statement; requires explicit `confirm: true`.   |

Every mutating tool applies the same safety scaffolding: allow-listed leading
keyword, exactly one statement, and execution via `sqlx::AssertSqlSafe` only
after those checks pass.

### `errors`
Single `AgentError` enum (via `thiserror`) in `src/errors.rs`. Covers
readline, terminal rendering, LLM/transport, context, tool, plan, and state
failures, plus the `ExceedMaxIter` sentinel returned when the agent loop
exhausts its budget.

### `config`
`AgentConfig { databases, state_path, max_iterations: 10, max_rows: 500 }` with
sensible defaults (`src/config.rs`). Reserved for future multi-database routing
and persisted conversation state under `.state/state.data`.

## Development

The `Makefile` mirrors the `ci` pipeline:

```sh
make build       # debug build
make release     # optimised build
make check       # cargo check --all-targets
make clippy      # clippy with -D warnings
make fmt         # rustfmt
make fmt-check   # formatting CI gate
make test        # cargo test --all-targets
make ci          # fmt-check + clippy + test
```

System prompts live in `docs/system_prompts.md` and are baked into the binary at
compile time; tweak them to change agent behaviour without code changes.

## Project layout

```
src/
├── main.rs              # entry: env -> LLM -> pool -> tools -> agent
├── lib.rs               # module declarations + SYS_PROMPTS constant
├── agent.rs             # PGAgent: REPL + per-turn tool-call loop
├── llm.rs               # OpenAI-compatible client (BYOT) + helpers
├── tool.rs              # Tool trait + ToolRegistry
├── context.rs           # system prompt loader
├── errors.rs            # AgentError thiserror enum
├── config.rs            # AgentConfig defaults
└── db/
    ├── mod.rs           # pool, registration, shared helpers
    ├── list_tables.rs   # list_schemas, list_tables
    ├── desc_table.rs    # desc_table (columns, PK, FKs, indexes)
    ├── search_schema.rs # search_schema (fuzzy table/column lookup)
    ├── execute_query.rs # execute_query (SELECT/WITH/EXPLAIN)
    ├── execute_dml.rs   # execute_dml (INSERT/UPDATE/DELETE/MERGE + preview)
    └── execute_ddl.rs   # execute_ddl (CREATE/ALTER/DROP/TRUNCATE)

docs/
├── system_prompts.md    # bundled into the system message
└── create_db.md         # example psql bootstrap
```
