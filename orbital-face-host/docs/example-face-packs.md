# Example Face Packs

The example packs prove that Face Pack v0 is a scriptable skin runtime rather
than a hardcoded orb renderer. They share no face-specific Rust code.

## Basic Orb

Path: `examples/basic_orb`

The reference implementation uses layered antialiased circles, glow, color
changes, thinking dots, and speaking bars. It is the default pack and the
clearest example of the Lua lifecycle.

## Pixel Pet

Path: `examples/pixel_pet`

A blocky desktop buddy built from rectangles:

- idle blinking;
- listening outline pulse;
- thinking indicator blocks;
- speaking mouth animation;
- happy bounce and sparkles;
- error shake and sweat;
- sleeping closed eyes and `zZ`.

It demonstrates character animation without image assets.

## Terminal Cube

Path: `examples/terminal_cube`

A dark terminal/monitor companion for developer workflows:

- idle `READY` screen and scanlines;
- listening input prompt and cursor;
- thinking computation blocks;
- speaking waveform;
- happy `[ OK ]` and check;
- red error diagnostics and glitch movement;
- dimmed suspended mode.

It demonstrates text, rectangular composition, and a non-cute visual identity.

## Minimal Dot

Path: `examples/minimal_dot`

A compact low-distraction companion:

- idle glow;
- listening pulse;
- thinking orbit;
- speaking audio-level pulse;
- happy sparkles;
- error shake and warning mark;
- dim sleeping state.

It demonstrates that a face pack can use a much smaller window and restrained
visual language.

## Run and test

Choose a pack:

```sh
cargo run -- --face examples/pixel_pet
```

Then send the standard event sequence:

```json
{"type":"state","state":"idle"}
{"type":"state","state":"listening","caption":"Listening..."}
{"type":"state","state":"thinking","caption":"Thinking..."}
{"type":"state","state":"speaking","audio_level":0.8}
{"type":"state","state":"happy","caption":"Done"}
{"type":"state","state":"error","caption":"Something failed"}
{"type":"state","state":"sleeping"}
```

Captions are accepted by the shared event contract. Individual packs decide
whether to render them.
