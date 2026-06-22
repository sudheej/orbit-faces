# Orbital Quick Capture v0

Quick Capture provides explicit Windows-first selected-text capture and
optional global hotkeys. It does not continuously monitor applications and
does not capture screenshots, OCR, keystrokes, or screen contents.

## Commands

### `/selection`

Captures the current text selection from the foreground Windows application
and stores it as a `selected_text` context item.

```text
/selection
/context
```

The runtime records:

- selected text;
- foreground window title, when available;
- process name and PID, when available;
- whether previous clipboard text was restored.

The source application must still own keyboard focus when capture occurs.
Global hotkeys are the reliable cross-application path. A terminal command can
capture a selection in the terminal itself or when command input is supplied
without moving focus from the source application.

### `/ask-selection <question>`

Captures selected text, stores it in context, and immediately asks the model:

```text
/ask-selection explain this error
```

If no question follows the command, Orbital uses:

```text
Explain this selected text briefly.
```

### `/ask-selection-once <question>`

Captures selected text and includes it only in the next model request. The
selection is not added to the persistent in-memory context list.

```text
/ask-selection-once summarize this privately
```

### `/ask <question>`

Explicit command form of a normal terminal prompt. It uses the current context
without capturing anything:

```text
/attach-text Build failed with missing class TaskRunMessageProcessor
/ask explain this
```

## Clipboard preservation

On Windows, selection capture:

1. reads and remembers existing text clipboard content when available;
2. sends a synthetic `Ctrl+C` to the foreground application;
3. waits briefly for the clipboard sequence number to change;
4. reads the selected UTF-8 text;
5. restores the previous text clipboard value.

If the previous clipboard was non-text, Orbital cannot reproduce that format
with the text-only clipboard layer. It prints a warning and leaves the selected
text on the clipboard. Capture failures do not crash the runtime.

## Active-window metadata

Windows metadata includes:

- title;
- process executable name when accessible;
- process ID;
- platform identifier.

Process-name lookup can fail because of process permissions. In that case the
title and PID are still returned when available, and the runtime logs a
diagnostic warning.

Non-Windows platforms report selection and active-window capture as unsupported
for now.

## Optional global hotkeys

Hotkeys are disabled by default. Enable them with:

```sh
cargo run --bin orbital-runtime -- --model mock --enable-hotkeys
```

Or with Ollama:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-model qwen2.5:1.5b \
  --enable-hotkeys
```

Windows hotkeys:

- `Ctrl+Alt+O`: print runtime status and ready message.
- `Ctrl+Alt+S`: capture selected text into context.
- `Ctrl+Alt+A`: capture selected text and ask the default question.

Registration requires no administrator privileges. If a shortcut is already
registered or global hotkeys are unsupported, Orbital prints a warning and
continues with terminal commands.

## Privacy model

- Selection capture occurs only after `/selection`, `/ask-selection`,
  `/ask-selection-once`, or an enabled selection hotkey.
- Hotkeys do not capture content until pressed.
- Watch mode remains metadata-only and refreshes only before prompts.
- All context is in memory and is never persisted.
- `/clear-context` removes stored selections and other context.

## Current limitations

- Native selection and hotkeys are Windows-only.
- Selection uses the conventional `Ctrl+C` mechanism, so applications that do
  not support copy cannot be captured.
- Rich clipboard formats cannot currently be restored.
- There is no GUI prompt window; hotkey questions use the default question or
  terminal input.
- There is no screenshot capture, OCR, voice, tools, MCP, or autonomous action.
