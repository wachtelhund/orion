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

## Automated QA (use this instead of manual playtesting)

`sim/qa.rs` + the `soak` example play N full bot games headless with
invariant checks (bounds, walkability, hp caps, resource overflow, field
leaks, economy stalls), shadow-determinism runs, and per-game metrics CSV.
`small_soak_is_clean` keeps a CI-sized version in `cargo test`. Workflow for
any gameplay change: run `soak -- 32` before and after; a violation or a
win-rate cliff is the signal. The `balance` example is the quick matchup
summary (`ORION_BALANCE_MAP=x` filters to one map). When a matchup is
lopsided, split combat from macro before touching numbers: `combat_lab`
pits equal-cost clumps on open ground (unit stats verdict — mind its
A-seat bias, compare swapped pairs), `probe_race r0 r1 seed` prints a
30s macro timeline (banked resources, army/building mix — this is how
Ferron's gas-hoarding leak was found). Client-side verification stays
`--script` / `--shot` / `--mp-auto`.

## Multiplayer

`sim/net.rs`: lockstep over newline-framed RON, transport-agnostic `Net`
(TCP threads, or WebSocket via `client/relay.rs`). The **lobby relay** is a
Cloudflare Worker + Durable Objects (`relay/` dir, `wrangler deploy`): one
`Lobby` DO per 5-letter code relays frames between two peers, and a
singleton `Directory` DO lists open public lobbies (`GET /lobbies`, JSON).
The UI never shows IPs: public lobbies appear in a browser (player name +
race, click to join, polled every 3s); private lobbies are joined by code —
the code IS the password. TCP direct-connect still exists for the
`--mp-auto host`/`join` loopback tests but has no UI. E2E lobby lifecycle
check: `--mp-auto host-pub:CODE` + curl `/lobbies` + `--mp-auto
join-relay:CODE`. Host binds DEFAULT_PORT,
handshake exchanges seed + races, then both sides run `Lockstep::try_step`:
local commands schedule INPUT_DELAY ticks ahead; a tick steps only when both
players' command lists for it are present; checksums cross-check every 24
ticks (desync = detected, not prevented). The loopback test
(`net::tests::lockstep_over_tcp_stays_in_sync`) and the two-process client
smoke (`orion-client --mp-auto host` / `--mp-auto join`) must stay green.
In MP the client never pauses, forces speed 1.0, runs no bot, and the local
player may be 1 (`App.human` — never hardcode player 0 in client code).

## Web build (wasm)

`web/build.sh` -> `web/dist`; serve with `web/serve.py` (no-cache — Chrome
caches `_bg.wasm` brutally hard otherwise). Port lore lives in
`web/README.md`. The sim compiles for wasm unchanged; client shims:
`crate::clock` + `net::nettime` (web-time), cfg-gated `process::id`,
localStorage settings, `relay_wasm.rs` (real WebSocket lobby MP driven by
a 16ms pump + fetch-based public lobby list; ranked still native-only —
the relay serves JSON with CORS `*` for browser clients). `?page=mp`
(also replays/difficulty) jumps straight to a menu page, which is how
headless-Chrome screenshots verify web UI without scripted clicks:
`chrome --headless=new --enable-unsafe-webgpu --virtual-time-budget=30000
--screenshot=x.png "http://localhost:8765/index.html?page=mp"`. Two rules learned the
hard way: **PROTOCOL_VERSION is CARGO_PKG_VERSION — rebuild `web/dist`
AFTER the version bump** or browser vs native refuses with a version
mismatch; and **hidden pages get no rAF and ~1Hz timers** — a
user-event proxy nudged by a 250ms interval calls `App::background_step`
so a backgrounded browser peer slows the match instead of freezing it.
`--mp-auto` runs open a small quiet window (never fullscreen, no audio,
no settings writes): fullscreen test windows get closed by the human at
the desk, which reads as a mystery disconnect in the logs.

## Maps

`map.rs` registry: `MAP_NAMES` + `by_name` — names are replay/net
identifiers, never rename a shipped one. Four maps: causeway (88x88,
central land-bridge plateau carrying two contested thirds; open flank
ramp + rock-sealed breach ramp per third; groves on the flanks) and the
original three: meridian (single-base,
mines out ~15 min by design), caverns (Xel'Naga Caverns homage with
natural expansions; `Map.expansions` holds suggested HQ origins and the bot
takes its natural at ~13 workers), and thornwood (96x96, contested
high-ground thirds, `Map.trees`/`Map.rocks` destructibles: RES_TREE blocks
movement AND line of sight — fog runs a Bresenham occlusion pass against
`State::vision_block`, skipped entirely on tree-free maps and for flyers —
RES_ROCK blocks movement only; both die by hp to explicit AttackTarget,
never auto-acquired). Every map MUST be exactly 180-degree
rotationally symmetric — run `cargo run -p orion-sim --example symmetry`
(terrain, elevation, resources, expansions, starts) and keep the sim
point-symmetric (see `examples/mirror.rs`, the mirror-covariance probe that
found the spawn bias: fixed-point Mul truncation, flow-field tie-break
mirroring, spawn-arc symmetry all live downstream of it). The MP handshake
carries the host's map choice; soak alternates maps per matchup cycle.

## Ranked matchmaking

`Matchmaker` DO on the relay (singleton): clients hold a WebSocket to
`/queue` while searching; a pass every 3s pairs players whose MMR gap and
combined relay RTT fit inside both players' windows (both widen with wait
time; continent mismatch adds a latency penalty). Matched players get an
M-prefixed code and reconnect through the ordinary Lobby relay — the
matchmaker only introduces. Ratings: Elo K=32 start 1200, stored per
anonymous `player_id` (random hex in settings); both clients report
`winner_slot` (host = slot 0), agreement resolves, single report resolves
after 120s (rage-quit protection), disagreement discards. Client flow
lives in `relay.rs::find_match_async` (one thread: RTT probe -> queue ws ->
lobby handshake). E2E: two `--mp-auto queue:X` processes + curl /result +
/rating. Humans only by construction — every queue entry is a live socket.

## Races

Three races: Vanguard Combine (0), Kyth Assembly (1), Ferron Compact (2).
Defs carry `race: u8`; players pick races; Train/Build validate race. The
bot is capability-driven (worker = harvester, supply = supply_provided,
tier-0/1 production by requires-chain) so new races need data + sprites
only — Ferron shipped with zero engine changes. Balance workflow: run the
`balance` example (full 3-race round-robin x maps x seeds with bot
personality styles) before and after any numbers change; `soak` cycles
the same 9 matchups. `--races A,B` aims `--shot`/`--record` captures at
any matchup.

Transform abilities share the siege toggle hotkey: Breakers siege,
Bulwarks deploy a 35% damage-soak aura (strongest single aura, applied
in `apply_damage`), Burrowers burrow (hidden + direct-fire-immune,
`Entity::burrowed` is checksummed; area damage carries an `area` flag on
hits and still connects; bot perception skips burrowed enemies).

## CI / releases

`.github/workflows/ci.yml` runs tests + an 8-game soak + the stress budget
on every push/PR, plus a Linux-only client build — private-repo Actions
bill macOS at 10x and Windows at 2x, and a 3-OS matrix per push burned
the monthly allowance in days (2026-08-05 billing lockout). The full
platform matrix is MANUAL: `release.yml` is workflow_dispatch-only —
Actions tab -> Release -> Run workflow with the tag (or `gh workflow
run release.yml -f tag=vX.Y.Z`). Tagging alone builds nothing; Hampus
decides when platform binaries get built. `release.yml` is manual
(workflow_dispatch with a tag input): macOS universal (lipo), Linux
x86_64, Windows x86_64, attached to the tag's GitHub release. Linux builds need `libasound2-dev` (rodio/ALSA) — that
is the only system dependency.

**Every minor and major version gets a wiki page** (mirrored into
`docs/wiki/` in the main repo — the GitHub wiki feature doesn't render
on private free-plan repos, so docs/wiki is what the user actually
sees; push BOTH) (the wiki is the
`orion.wiki.git` repo next to the main one). Each page covers: what's new
and changed (features AND balance diffs, explained — not just a commit
list), plus in-game screenshots of the new content (`--shot`/`--menu-shot`
captures, committed into the wiki repo and referenced relatively). Write
the page as part of cutting the tag, and link it from a `Home` index page
of all versions.

## Known gaps / next milestones (see SPEC.md for full list)

1. 2v2: lockstep generalizes to N players, but lobby/matchmaker/UI don't
   yet.
2. macOS builds are unsigned — Gatekeeper right-click-open. Needs an
   Apple Developer ID in CI.
3. VC still leads the cross-race Hard round-robin (meridian: VC 25 /
   Kyth 15 / Ferron 8 after the v0.17.1 macro fixes; was 27/14/6, and
   VC-vs-Ferron went 15-1 -> 12-4). Bot macro is no longer the whole
   story — next lever is unit numbers, which deserves a dedicated
   tuning pass; 8-seed sweeps are noisy, don't over-tune on one.
4. Hard-bot mirrors on thornwood mostly hit the 20-minute balance
   budget (all races — turtling on the multi-base map), so the balance
   example's thornwood rows are mostly stalls. Normal-difficulty soak
   games finish fine.
5. Web build parity: the ranked queue and replay UPLOAD are
   desktop-only (the browser lists/joins lobbies since v0.19.0 and
   fetches/plays shared replays since v0.20.0 — `?replay=CODE` links
   work); a hidden browser tab plays in slow motion by design
   (background keep-alive) rather than stalling the peer.
