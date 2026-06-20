# Orbital Face Host - Agent Instructions

This repository is a narrow prototype for a desktop companion face host.

## Product intent

Orbital is a local-first desktop companion system. This repo only handles the visual face host: a small skinnable orb/character window that can animate based on external state.

The long-term product may include LLMs, voice, tools, agents, and a marketplace, but those are not part of this repo yet.

## Scope for this repo

Allowed:
- Rust + SDL3 windowing experiments
- transparent/borderless/shaped window experiments
- click-through and hitmask experiments
- Lua scripting for face behavior through `mlua`
- simple state event handling through stdin JSON
- example face packages
- documentation of platform limitations

Not allowed:
- LLM integration
- OpenAI/Ollama integration
- speech-to-text
- text-to-speech
- marketplace
- account system
- cloud backend
- plugin security framework
- complex agent orchestration

## Engineering principles

- Keep the prototype small.
- Prefer working vertical slices over abstractions.
- Document platform-specific behavior honestly.
- Windows is the first target.
- Avoid becoming a general game engine.
- The API should feel Lua/LÖVE-inspired but be companion-specific.
- Face scripts should receive state and draw; they should not own the backend.
- If unsafe Rust or `sdl3-sys` is required, isolate it in a small platform-specific module.

## Desired face API shape

The Lua face API should eventually feel close to:

function companion.load()
end

function companion.state_changed(state)
end

function companion.update(dt)
end

function companion.draw(ctx)
end

function companion.hit_test(x, y)
end

## Current acceptance goal

A user can run the app, see a small animated orb, drag it, toggle always-on-top, and send state changes from the terminal.