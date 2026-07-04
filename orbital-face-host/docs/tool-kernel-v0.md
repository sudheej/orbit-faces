# Orbital Tool Kernel v0

The tool kernel is a synchronous backend layer owned by `orbital-runtime`.
`orbital-face-host` remains a visual surface and contains no tool, model, or
memory logic.

Every built-in tool is registered with a description, JSON input schema, risk
level, confirmation policy, local-only flag, and read-only flag. Unknown tools
and invalid arguments are rejected before execution. A tool marked non-local
is denied by the v0 policy.

## Built-in tools

| Tool | Risk | Confirmation | Behavior |
| --- | --- | --- | --- |
| `time.now` | low | no | Local Unix timestamp and timezone label |
| `context.list` | low | no | Current context summaries |
| `context.get` | low | no | One current context item by ID |
| `active_window.get` | low | no | Supported active-window metadata |
| `clipboard.read` | medium | no | Read clipboard text requested by the user |
| `memory.remember` | medium | no | Store explicit user-requested memory |
| `memory.search` | low | no | Local text search |
| `memory.list` | low | no | Recent explicit memories |
| `memory.forget` | medium | no | Soft-delete memories matching text |
| `face.set_state` | low | no | Internal/demo face state event |
| `file.read_text` | medium | no | One UTF-8 file, maximum 64 KB |

`file.read_text` rejects directories, missing files, binary-looking content,
invalid UTF-8, empty files, and files above the size limit.

There is no shell, file mutation, network fetch, browser control, email,
calendar mutation, Git mutation, downloaded plugin, or background executor.

## Manual execution

```text
/tools
/tool-info time.now
/tool time.now {}
/tool context.list {}
/tool memory.search {"query":"TaskRunMessageProcessor"}
/tool file.read_text {"path":"./README.md"}
/tool-history
/tool-history 5
/tool-clear-session
```

Current low/medium local-only built-ins execute directly because the current
user request is treated as authorization. Future high-risk tools will place the
face in `thinking` with `Approval needed` and require terminal confirmation.
The face uses the existing `thinking`, `happy`, `error`, and `idle` states.

All proposed, denied, executed, and failed invocations are stored in the local
`tool_invocations` audit table with arguments, source, risk, confirmation flag,
result or error, and timestamps. If the audit entry cannot be created, the tool
does not execute.

## Model-suggested tools

Model tools are disabled by default. Enable them explicitly:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-model qwen2.5:1.5b \
  --enable-model-tools
```

For Ollama, the runtime sends the registered function names, descriptions, and
argument schemas through Ollama's dedicated `tools` request field. Tool calls
return separately from assistant content, so tool-control JSON never reaches
face captions or TTS. The runtime rejects multiple calls, unknown names, and
arguments that fail the registry schema.

The normal assistant system prompt includes a short names-and-descriptions
catalog, so the model can accurately answer capability questions without
running the tool planner or exposing schemas.

To keep continuous conversation responsive, the tool-selection pass runs only
for explicit actionable intents such as time, clipboard, active-window, memory
write/search, context, or file requests. Capability questions and ordinary
memory recall use the normal response path.

The runtime executes at most one low/medium local tool and sends its result
through one follow-up model request. Tool suggestions are disabled for that
follow-up, preventing chains. Future high-risk tools require terminal approval,
and auto-listen pauses while approval is pending. There is no hidden,
background, or multi-step execution; every call remains visible in terminal
output and the audit log.

The mock provider has a deterministic test hook: with model tools enabled, a
prompt containing `search memory` suggests `memory.search`. A strict
fenced-block parser remains covered for backward-compatible
input tests, but generated control data no longer shares the speech response.
