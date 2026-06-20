# Windowing Notes

This prototype is Windows-first. Other platforms may compile later, but they are not a product target yet.

## Borderless Window

Status: implemented through SDL3 `WindowBuilder::borderless` and `WindowFlags::BORDERLESS`.

Expected Windows behavior: the window should have no normal frame or title bar.

## Transparent Background

Status: attempted.

The window is created with SDL3's `WindowFlags::TRANSPARENT`, and the Lua script clears with transparent black. Actual per-pixel transparency depends on the SDL3 backend, renderer, and Windows compositor path.

If the window appears black instead of transparent, this prototype still proves Lua-driven drawing and borderless movement, but the transparency path needs a Windows-specific rendering/composition pass.

## Shaped Window / Alpha Mask

Status: implemented through SDL3, pending Windows verification.

The prototype creates an RGBA surface containing a circular alpha mask and
passes it to `SDL_SetWindowShape`. The unsafe `sdl3-sys` call is isolated in
`src/platform/mod.rs`. SDL copies the mask, so the temporary surface can be
released immediately.

The mask is static and intentionally slightly larger than the normal orb. Face
scripts should keep visible drawing inside that boundary unless the manifest
and host API are extended later.

## Click-Through Outside The Orb

Status: implemented through the SDL shape mask, pending Windows verification.

SDL documents fully transparent pixels in a shaped window as transparent to
mouse clicks. The prototype also installs a hit-test callback and only starts
its manual drag fallback inside the same circular mask.

If the selected Windows SDL renderer/backend does not honor this behavior, the
fallback is a Win32 region or layered-window implementation isolated in
`src/platform/windows.rs`. That fallback is not included until real Windows
testing demonstrates it is needed.

## Dragging

Status: implemented.

The visible orb area can be dragged. SDL hit-test requests draggable behavior, and the app also includes manual drag handling as a fallback.

## Always-On-Top

Status: attempted on Windows only.

Press `T` to toggle. The prototype isolates the native call in `src/platform/windows.rs`. This needs verification on a real Windows build because SDL3's high-level Rust window API does not expose every runtime window flag yet.

Because the window requests `NOT_FOCUSABLE`, keyboard events are not a reliable
control mechanism. `{"always_on_top":true}` and
`{"always_on_top":false}` can also be sent through stdin.

## Keyboard Focus

Status: attempted.

The window is created with SDL3's `NOT_FOCUSABLE` flag. On Windows, this should reduce focus stealing, but behavior can vary when a borderless window is clicked or dragged. Verify against normal desktop apps before relying on it.

## Current Verdict

The requested SDL3 implementation paths are present: transparent window,
circular shape mask, non-focusable flag, draggable visible region, and
always-on-top control. The prototype is ready for Windows acceptance testing,
but shaped transparency, click-through, and focus behavior are not considered
verified until exercised on a real Windows desktop.
