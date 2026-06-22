# Future Winamp Panel Face Pack

`winamp_panel` is intentionally deferred.

The intended pack is a compact, irregular desktop panel inspired by classic
Winamp skins: status display, waveform, tiny controls, and a non-circular
drag/click region.

Before adding it, the runtime should support:

- manifest-declared hitmask assets or polygonal regions;
- non-circular Wayland input regions;
- equivalent Windows and macOS click-through masks;
- optional image asset loading;
- a clear distinction between draggable and interactive control regions.

Face Pack v0 currently proves script-defined visuals and simple hit testing.
Adding `winamp_panel` now would imply stronger cross-platform shape guarantees
than the runtime provides.
