# Orbital Context v0

Orbital Context v0 provides explicit, session-local context for model prompts.
The runtime owns collection and prompt assembly; the face host only displays
short state captions.

Context is never persisted. Orbital does not capture screenshots, record the
screen, scan projects, read files recursively, or poll the clipboard.

## Commands

- `/clipboard`: read the current text clipboard once and attach it.
- `/active-window`: attach the foreground window title and process on Windows.
- `/watch`: explicitly enable active-window metadata refresh before prompts.
- `/unwatch`: stop refreshing active-window metadata.
- `/attach-text <text>`: attach a terminal-provided text snippet.
- `/attach-file <path>`: attach one local UTF-8 text file.
- `/context`: list current items, source kinds, sizes, and watch status.
- `/clear-context`: remove all context and disable watch mode.

`/clear` remains separate and clears only conversation history.

## Examples

```text
/clipboard
summarize this
```

```text
/attach-text Build failed with NoClassDefFoundError in TaskRunMessageProcessor
explain this
```

```text
/attach-file ./README.md
what does this project do?
```

On Windows:

```text
/watch
what window am I working in?
/unwatch
```

Selected text is handled by Quick Capture. See
[quick-capture-v0.md](quick-capture-v0.md).

## Clipboard privacy

Clipboard access occurs only when `/clipboard` is entered. Orbital reads text
only. Empty, unavailable, or non-text clipboards return an error and do not
replace existing context.

## Active-window privacy

Active-window support is Windows-first. It records only:

- foreground window title;
- foreground process executable name.
- foreground process ID.

It does not capture pixels, OCR text, keystrokes, file contents, or application
documents. Watch mode does not run a background polling loop; after `/watch`,
the metadata is refreshed only immediately before a user prompt. On non-Windows
platforms, active-window commands return a clear unsupported error.

## File guardrails

`/attach-file`:

- accepts one regular file path;
- reads at most 64 KB;
- requires valid UTF-8 text;
- rejects empty and binary-looking files;
- does not recurse into directories;
- never executes the file.

## Prompt assembly

Attached items are formatted as:

```text
[Orbital Context]
Context item 1:
Type: attached_text
Title: Attached Text
Source: terminal
Content:
...

[User Request]
...
```

The combined context block is capped at 12,000 characters. If the limit is
reached, Orbital inserts `[Context truncated]`. Conversation history and the
user request remain separate from this budget.

## Face feedback

Collection commands use short visual states:

- `thinking` while reading clipboard, files, or active-window metadata;
- `happy` when context is attached or watch state changes;
- `error` when collection fails;
- `idle` when ready again.

## Limitations

- Context is in memory for the current runtime process only.
- Clipboard snapshots do not update automatically.
- Watch mode is metadata-only and Windows-only in v0.
- There is no screenshot capture, OCR, vector indexing, MCP, or autonomous
  context collection. Explicit local memory and read-only tools are documented
  separately.
