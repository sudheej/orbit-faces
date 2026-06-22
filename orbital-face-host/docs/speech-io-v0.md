# Orbital Speech I/O v0

Speech I/O is explicit push-to-talk only. Orbital does not use a wake word,
continuously monitor the microphone, perform background recording, or run VAD.

## Providers

### Speech-to-text

- `mock` is the default. It skips microphone recording and returns
  `mock voice prompt` deterministically.
- `whisper` invokes an external whisper.cpp `whisper-cli` process with a local
  ggml model.

### Text-to-speech

- `none` is the default and never accesses speakers.
- `piper` invokes a configured Piper binary, creates a temporary WAV, and uses
  the platform playback command.
- `windows-sapi` invokes Windows System.Speech through PowerShell.

Tests require none of these external programs, model files, microphones, or
speakers.

## Commands

### `/listen [seconds]`

Runs one explicit capture, from 1 to 20 seconds. The default is 5 seconds.

```text
/listen
/listen 8
```

The face shows `listening`, then `thinking` while STT runs. A successful
transcript enters the existing model prompt flow and produces speaking
captions.

In mock mode, no microphone is opened:

```sh
cargo run --bin orbital-runtime -- --model mock --stt mock --tts none
```

### `/transcribe-file <path>`

Transcribes an existing WAV and prints the result without sending it to the
model:

```text
/transcribe-file ./sample.wav
```

### `/say <text>`

Speaks text through the selected TTS provider:

```text
/say Hello, I am Orbital.
```

With `--tts none`, Orbital prints guidance and remains running.

### `/speech-status`

Prints the selected STT/TTS providers, configured model and Piper paths,
microphone availability, response-speaking setting, last transcript, and last
speech error.

## Whisper setup

Build or install whisper.cpp and manually download a compatible model such as
`ggml-tiny.en.bin`. Orbital does not download binaries or models.

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --ollama-model qwen2.5:1.5b \
  --stt whisper \
  --whisper-bin ./whisper.cpp/build/bin/whisper-cli \
  --whisper-model-path ./models/ggml-tiny.en.bin \
  --tts none
```

Then:

```text
/listen 5
```

The whisper.cpp sidecar receives:

```text
whisper-cli -m <model> -f <wav> -np -nt -l en
```

## Piper setup

```sh
cargo run --bin orbital-runtime -- \
  --tts piper \
  --piper-bin ./piper \
  --piper-model ./voices/en_US-lessac-medium.onnx
```

Optional:

```text
--piper-config ./voices/en_US-lessac-medium.onnx.json
```

Piper WAV playback uses PowerShell on Windows, `aplay` on Linux, and `afplay`
on macOS. Missing playback commands produce a runtime error without terminating
Orbital.

## Windows SAPI

```sh
cargo run --bin orbital-runtime -- --tts windows-sapi
```

```text
/say Hello, I am Orbital.
```

Non-Windows platforms report Windows SAPI as unsupported.

## Speaking model responses

Add `--speak-responses` to speak completed model answers:

```sh
cargo run --bin orbital-runtime -- \
  --model ollama \
  --tts windows-sapi \
  --speak-responses
```

Spoken responses are limited to 500 characters. TTS errors are warnings and do
not fail the model response or caption flow.

## Hotkey

When Windows global hotkeys are enabled, `Ctrl+Alt+L` triggers `/listen 5`.
Existing hotkeys remain unchanged.

## Audio and privacy behavior

- Microphone capture occurs only for `/listen` or `Ctrl+Alt+L`.
- Capture duration is always bounded from 1 to 20 seconds.
- Audio is written to a temporary PCM WAV and removed after transcription where
  practical.
- `/transcribe-file` never modifies the source file.
- No recording starts at runtime startup.
- No wake word, VAD, continuous STT, cloud speech service, or persistent audio
  storage exists in v0.

## Limitations

- Whisper is a sidecar process rather than an in-process library.
- STT is batch transcription, not streaming.
- Piper playback depends on an OS playback command.
- Device permissions and audio backend setup are platform-specific.
- Windows SAPI and real Windows audio still require native validation.
