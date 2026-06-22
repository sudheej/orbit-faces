# Windowing Notes

Window behavior is capability-based because desktop stacking, focus,
transparency, and click-through are platform services rather than portable
rendering features.

## Hyprland / wlroots Wayland

Status: implemented and visually verified on Hyprland.

- Borderless: yes. The host is a `wlr-layer-shell` surface, not a decorated
  `xdg_toplevel`.
- Transparency: yes, through premultiplied ARGB shared-memory buffers.
- Always-on-top: Overlay is used by default so the face remains visible.
  Runtime off switches to Top, which may place it behind normal Hyprland
  windows.
- Focus: keyboard interactivity is `OnDemand`. The face does not request focus
  at startup, but clicking it can grant focus for A/D/Esc testing.
- Dragging: implemented by changing top/left layer margins while dragging.
- Click-through: yes outside the circular Wayland input region. Lua may further
  restrict drag initiation through `companion.hit_test`.
- Configuration: no Hyprland configuration changes are required.

This backend requires `wlr-layer-shell`; it is not universal across every
Wayland compositor.

## Windows SDL3 path

Status: implemented but not validated on a real Windows desktop.

- Borderless: requested through SDL3 flags.
- Transparency: requested through `SDL_WINDOW_TRANSPARENT`.
- Shape/click-through: a circular alpha surface is passed to
  `SDL_SetWindowShape`. SDL documents fully transparent shape pixels as
  click-through.
- Always-on-top: toggled through isolated `sdl3-sys` usage.
- Focus: `SDL_WINDOW_NOT_FOCUSABLE` is requested.
- Dragging: the internal circular hit test limits manual dragging.

Renderer, compositor, and Windows-version differences still require real
acceptance testing. If SDL3 does not provide reliable layered-window behavior,
the fallback belongs in `src/platform/windows.rs`, not in the shared runtime.

## macOS

Status: not validated and no AppKit-specific fallback exists yet.

The SDL3 path may provide borderless transparency, but focus, click-through,
window level, and shape behavior must be tested on macOS before support is
claimed.

## Keyboard controls

- `A`: toggle always-on-top
- `D`: toggle debug overlay
- `Esc`: quit

The same controls are available through stdin/test-script commands when the
window intentionally has no keyboard focus.

## Hitmask

`src/hitmask.rs` currently implements a circular default mask. It is used for
Wayland input regions and drag initiation. Face Pack v0 does not yet declare a
hitmask asset; that extension can be added without changing the current
runtime contract.
