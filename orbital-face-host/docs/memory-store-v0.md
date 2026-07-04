# Local Memory Store v0

Orbital Runtime owns one local SQLite database. The face host never opens the
database and receives no memory contents except captions explicitly sent by the
runtime. SQLite is bundled through `rusqlite`; there is no remote database,
cloud synchronization, account, or network access.

## Database location

Defaults:

- Windows: `%APPDATA%/Orbital/orbital.db`
- Linux with `XDG_DATA_HOME`: `$XDG_DATA_HOME/orbital/orbital.db`
- Other Linux environments: `~/.local/share/orbital/orbital.db`

Override the path for one run:

```sh
cargo run --bin orbital-runtime -- --db-path ./orbital.db
```

`ORBITAL_DB_PATH` is also accepted when `--db-path` is absent. If the configured
database cannot be opened or migrated, the runtime reports the error and uses a
temporary in-memory database for that process instead of terminating.

## Persistence defaults

The current configuration shape is:

```toml
[memory]
enabled = true
store_session_messages = false
store_context_items = false
db_path = null
```

There is not yet a general TOML configuration loader. `--db-path`,
`--store-session-messages`, and `--store-context-items` map to these fields.
Explicit user memories and tool audit entries are stored by default. Full
session messages and context item contents are not stored unless their flags
are supplied. Raw microphone and TTS audio are never stored.

Recent retained facts are loaded into model context on every prompt, including
after the runtime starts a new session. They are labeled as `Continuity
knowledge` without database IDs or tool terminology, so the companion can use
shared history naturally. Storage mechanics should only be discussed when the
user asks about memory controls, privacy, or implementation.

`/memory-off` disables new user-memory and optional session/context writes for
the current process. Tool audit writes remain mandatory so execution is never
hidden. `/memory-on` re-enables memory writes.

## Commands

```text
/remember User is building Orbital as a local-first desktop companion.
/memories
/memory-search Orbital
/forget-memory 1
/memory-status
/memory-clear-session
/memory-off
/memory-on
```

`/remember` is the unambiguous manual write path. Natural requests such as
"remember that ..." and a constrained set of direct family/profile facts also
route through the same audited memory tool. General background conversation is
not silently retained. `/forget-memory` sets `deleted_at`; it does not destroy
the row. `/memory-clear-session` deletes the current session's stored messages,
context records, and tool audit records, but preserves `user_memories`.

## Schema and future work

The schema contains `sessions`, `messages`, `context_items`, `user_memories`,
`tool_invocations`, `settings`, and migration metadata. Search is a
case-insensitive escaped SQL `LIKE` query over active memory content. There are
no vector embeddings yet.

A future implementation may place the `MemoryStore` trait over embedded
libSQL/Turso and add an isolated vector index. Remote Turso, cloud sync, and
general model-driven memory extraction is explicitly outside v0.
