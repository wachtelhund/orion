# Orion — Architecture & Domain Context

StarCraft-style 1v1 RTS. Rust, no engine. Read SPEC.md first — it records the
product decisions and their rationale; this file records how the code delivers
them.

## The one rule that outranks everything

**The sim must be bit-deterministic.** Two peers stepping the same commands
must produce identical `State::checksum()` at every tick, on every platform.
Multiplayer is deterministic lockstep; replays are input streams. Any PR that
breaks `crates/sim/tests/determinism.rs` is wrong by definition.

Concretely, inside `orion-sim`:

- **No floats in sim logic.** All math is `Fx` (Q16.16 fixed-point,
  `fixed.rs`). `f64` appears only in data loading (decimal → fixed conversion
  is IEEE-exact everywhere). The renderer may use floats freely.
- **No `HashMap`/`HashSet` iteration.** Vec scans in index order, or ordered
  structures. Ties break by entity index — explicitly, everywhere.
- **No wall clock, no OS RNG, no thread timing.** Randomness only via
  `SimRng` inside `State`.
- The renderer never mutates state. The **only** mutation channel is
  `State::step(&[(PlayerId, Command)])` — exactly what will go over the wire.

## Crate layout

```
crates/sim      orion-sim     deterministic simulation (no gfx deps)
crates/client   orion-client  winit + wgpu renderer, input, HUD, bot driver
```

### sim internals (tick pipeline order is load-bearing)

`step.rs`: commands → production → behavior → damage/deaths → movement →
fog → victory → field GC. Don't reorder.

- `state.rs` — one flat `Entity` struct for units/buildings/resources (SC1
  did the same; three arrays would cost more bookkeeping than it saves).
  Generational `EntityId` catches stale references.
- `path.rs` — flow fields: one Dijkstra per move *order*, shared by every
  unit in it. Terrain is static; buildings enter via a dynamic `blocked`
  grid. Fields are pooled and mark-swept every 24 ticks.
- `movement.rs` — desired velocities + pairwise soft separation (i<j once,
  symmetric push) + axis-separable terrain sliding. Not RVO (float-heavy,
  order-sensitive).
- `combat.rs` — behavior pass emits damage events; application is a separate
  pass (no mid-iteration mutation of other entities).
- `econ.rs` — gather loop (ToResource → Mining → ToDepot) for minerals AND
  gas (extractor buildings are gather targets; they consume their geyser on
  construction and inherit its amount), plus SCV-style worker construction.
  Cost is deducted when construction *starts*.
- `fog.rs` — per-player visible/explored grids, stamped sight circles,
  high-ground rule: you see a tile only if its elevation ≤ yours.
- Content systems: flyers (straight-line movement, terrain-immune, air-only
  targetable by `air` weapons, see over cliffs), siege mode (`weapon_siege`,
  splash w/ friendly fire, min range, anchored while deployed), spellcaster
  energy + Plasma Storm zones (`State::storms`, checksummed), research
  (per-building `research` slot -> player weapons/armor levels applied in
  `upgraded_damage`), tech-tree `requires` on units/buildings.
- `ai.rs` — the bot is a *command source*: reads state, emits `Command`s,
  same channel as a human. Keep it that way — it's what makes it
  deterministic, replay-compatible, and lockstep-compatible. Three
  difficulties parameterize macro only (no cheating). Bots gate their own
  think interval — call `think` every tick.
- Units carry a shift-queue of orders (`order_queue`); `issue_order` /
  `finish_order` are the only entry points. Flow-field GC scans queues too.
- All balance content lives in `assets/units.ron` (seconds / tiles-per-second
  units; compiled to ticks/fixed at load). Adding units or race #2 should
  touch data + maybe `ai.rs`, not the engine.

### client internals

- `atlas.rs` — ALL art is procedural pixel-art painted at startup into a
  1024² atlas: dithered terrain tile variants, cliff wall faces, mineral
  crystal clusters (3 depletion states), units at 8 facings × 2 walk frames
  × 2 teams (worker/trooper/vanguard painters), iso-box buildings with lit
  windows + team trim, effect sprites, 5×7 font. Style rules live at the top
  of the file: dark outlines, 3-4 tone ramps, light from upper-left, hard
  team accents. Sprites are baked per team — no runtime palette swaps.
- `gfx.rs` — one pipeline, one instanced draw per frame; instances support
  rotation (tracers/beams). Atlas is sRGB; instance colors are converted
  sRGB→linear in the shader so authored values round-trip exactly.
- `iso.rs` — fixed 2:1 projection, `ELEV_PX` per height level (cliff sprites
  are painted 2× this). Camera never rotates (SPEC decision).
- `app.rs` — fixed 24 Hz sim timestep, render-rate interpolation via
  `prev_pos`; input → `Command`s appended to `pending`, drained next tick.
  Sim `events` (attacks/deaths) spawn client-side effects: muzzle flashes,
  tracers, sparks, blast rings, corpses, rubble, command pings. Facings are
  a render-side cache derived from movement deltas. SC-style bottom console:
  minimap, portrait, multi-select strip, command card.
- `config.rs` — settings + rebindable hotkeys persisted to
  `~/.orion-settings.ron`. `menu.rs` — main/pause/settings pages; layout is
  shared between draw and hit-testing. `hud.rs` — console (minimap, portrait,
  production queue with click-cancel, clickable command card); every metric
  scales by `App::ui()` = display DPI × the HUD-size setting.
- `audio.rs` — ALL sound is synthesized at startup (rodio playback): SFX and
  the ambient music loop. `App::deny` = flashing mid-screen warning + error
  sound; sim completion events (Ready/BuildingDone/ResearchDone) drive
  chimes. Combat sounds throttle per tick. Audio init is skipped in headless
  capture modes and failure to open a device is never fatal.
- Tooltips: every HUD element (card buttons, queue slots, strip tiles) shows
  a hover panel built from data `desc` fields + live stats; button labels
  stay short and truncate — the tooltip carries the detail.
- Session flow: `in_game` + `MenuPage`. Menus pause the sim (single-player).
  Fullscreen requests go through `want_fullscreen` (applied by the shell,
  which owns the window).
- Cursor positions are quantized to 1/256 tile in `fx()` before entering the
  sim so cursor floats can't leak nondeterminism into the command stream.

## Verification loops (use them)

```sh
cargo test --workspace                                  # incl. determinism
cargo run --release -p orion-sim --example stress       # 200-unit tick budget
cargo run --release -p orion-sim --example botgame      # macro-loop printout
cargo run --release -p orion-client -- --smoke          # render stack up?
cargo run --release -p orion-client -- --shot s.ppm --shot-ticks 6480 \
    [--shot-zoom 3.0] [--shot-focus 40,40] [--shot-reveal]
cargo run --release -p orion-client -- --script prefix  # scripted human test
```

`--shot` verifies visuals without a human: fast-forwards a bot-vs-bot game,
captures one frame, exits (zoom/focus/fog-reveal overrides for close-ups and
map overviews). `--script` goes further: it plays through the *human command
path* — gather, train queue, build placement + construction, multi-select,
attack-move — captures a frame series at each stage, and asserts the base
actually got built. It caught the resume-construction bug; keep it passing.
Convert captures with `sips -s format png s.ppm --out s.png`.

## Known gaps / next milestones (see SPEC.md for full list)

1. **Netcode**: lockstep transport + input delay, thin matchmaker (axum +
   SQLite), P2P with relay fallback. The sim is already shaped for it.
2. **Replays**: serialize seed + command stream; nearly free, huge desync-
   debugging value. Do this *before* netcode.
3. Sprite pipeline: runtime consumes an atlas format; AI-generated sheets vs
   Blender renders is an open experiment (SPEC risk #5).
4. Bot reads map start locations (fair) but its difficulty tiers, scouting,
   and defense reactions are thin.
5. Balance is placeholder. Bot-vs-bot currently favors player 1 slightly —
   unexplained; investigate before reading anything into balance changes.
