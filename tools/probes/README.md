# Play probes

Luau chunks for `rustudio PLACE.rbxl --play-only --no-render --luau server FILE@SECONDS`
(or `--luau client`), used with `--profile-capture` to measure what a
game does under load (docs/ANDROID.md "Frame time on a phone",
docs/PROFILING.md). A chunk runs at command-bar identity with its own
copies of ModuleScripts; a probe that needs the game's module state
spawns a Script (voxel1.luau).

| file | side | what |
|---|---|---|
| `burst.luau` | server | 400 falling debris parts, 2000 anchored parts, 10 VFX parts a Heartbeat for 5 s; prints when the debris has come to rest |
| `fpswatch.luau` | client | fps and instance count every second (`[watch]`) |
| `propset.luau` | server | the cost of property writes from Lua on 4000 anchored parts (`[propset]`) |
| `voxel1.luau` | server | one Infernal-Blades-sized voxel hit at the player's feet (LBG; the character must stand on a `VoxelOrigin` part) |
| `trace-window.py` | — | `trace-window.py TRACE.json T0 T1 [TOP] [LANE]`: per-lane self/incl time of the scopes inside a window (seconds) of `--profile-convert` output |
