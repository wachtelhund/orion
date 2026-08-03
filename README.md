# Orion

A StarCraft-style RTS built from scratch in Rust. No game engine — a
purpose-built deterministic simulation with a wgpu isometric renderer.

See [SPEC.md](SPEC.md) for the full product specification and
[CLAUDE.md](CLAUDE.md) for architecture.

## Status

Playable single-player game:

- Full macro loop: minerals + plasma (gas) → build → tech → army → destroy
  the enemy base
- One race (Vanguard Combine), authored entirely in
  `crates/sim/assets/units.ron`:
  - Units: Fabricator (worker), Trooper, Vanguard (melee), **Breaker (siege
    tank with deploy mode + splash)**, **Skywing (flyer, hits air+ground)**,
    **Stormcaller (spellcaster: Plasma Storm AoE)**
  - Buildings: HQ, Supply Pylon, Muster Hall, Plasma Condenser, **Forge
    (tanks)**, **Aerie (flyers)**, **Archive (research: Weapons/Armor +1/+2)**
  - Tech tree requirements (Forge needs Muster Hall, Aerie needs Forge,
    Stormcaller needs Archive)
- One map ("Meridian"): two high-ground mains, ramps, chokepoints, geysers,
  fog of war with the high-ground vision rule
- Main menu (vs AI at three difficulties), pause menu with settings:
  fullscreen, HUD size, game speed, edge scroll, rebindable hotkeys
  (persisted to `~/.orion-settings.ron`)
- Skirmish bot at Easy / Normal / Hard (no cheating — difficulty is macro
  tightness and aggression timing)
- Order queueing (shift), control groups, ctrl+click type-select, rally
  points with auto-gather, production queue with cancel/refund
- Hover tooltips on every HUD element (units, buildings, research, actions)
- Procedural audio: ambient music loop + combat/UI/economy sound effects,
  all synthesized at startup (no asset files); volume sliders in settings
- Flashing warnings with error sounds: NOT ENOUGH MINERALS / PLASMA /
  SUPPLY / ENERGY, CANNOT BUILD THERE, REQUIRES <building>
- Deterministic lockstep-ready sim (fixed-point math, checksums, input-only
  mutation) — multiplayer netcode not wired up yet (menu stub explains)

## Run

```sh
# Play vs the bot
cargo run --release -p orion-client

# Headless bot-vs-bot game with status log
cargo run --release -p orion-sim --example botgame

# 200-unit pathing stress benchmark
cargo run --release -p orion-sim --example stress

# Render-stack smoke test (opens window 3s, exits)
cargo run --release -p orion-client -- --smoke

# Screenshot of a bot game at tick N (visual verification without a human)
cargo run --release -p orion-client -- --shot out.ppm --shot-ticks 6480 \
    [--shot-zoom 3.0] [--shot-focus 40,40] [--shot-reveal]

# Scripted human-play test: builds a base through the real command path,
# captures a frame series, asserts the buildings went up
cargo run --release -p orion-client -- --script prefix
```

All art is generated at startup — procedural SC1-style pixel sprites (units
with 8 facings and walk animation, structured buildings, cliff faces, mineral
crystals). There are no asset files to install.

## Controls

All game hotkeys are rebindable in Settings (Escape menu). Defaults:

| Input | Action |
|---|---|
| Left click / drag | Select / box-select |
| Ctrl+click / double-click | Select all units *or buildings* of that type on screen |
| Shift + click | Add/remove a unit from the selection |
| Shift + right click | Queue waypoint orders (also queues worker builds) |
| Right click | Move / attack / gather / resume construction / set rally |
| A + click | Attack-move |
| S / H | Stop / Hold position |
| B, then Q/W/E/R | Build grid: Pylon / Muster Hall / HQ / Condenser (SC2-style card with icons) |
| Q / W | Train (fills the shortest queue across selected buildings) |
| Click queue icon | Cancel queued unit (full refund) |
| Ctrl+# / Shift+# / # | Set group / add to group / select group |
| # twice | Jump camera to the group |
| Space | Jump to last under-attack alert |
| Backspace | Cycle your bases |
| F1 | Select idle worker |
| Arrows / edge / middle-drag | Pan camera (bounded to the map) |
| Scroll | Zoom |
| D | Siege / unsiege Breakers |
| E | Plasma Storm (Stormcaller selected), then click target |
| Tab | Cycle subgroup in a mixed selection |
| X | Cancel construction (75% refund) |
| Esc | Cancel mode, or open the pause menu |
| R | Restart (after game over) |
| F3 | Toggle fog reveal (dev) |

Economy: minerals from crystal fields; **plasma (gas)** from a Condenser built
on a geyser — advanced units cost both. Rally a production building onto a
resource and new workers start harvesting automatically. Fog is SC2-style:
terrain is always visible (dimmed), units and enemy buildings only when scouted.

## Test

```sh
cargo test --workspace
```

The tests that matter most: `determinism.rs` proves bit-identical simulation
from identical inputs (the property lockstep multiplayer stands on), and the
`stress` example enforces the sim tick budget with 200+ units.

## Deploy

Desktop binaries only, no infrastructure yet. `cargo build --release` produces
`target/release/orion-client` for the host platform. The matchmaker service
(1v1 queue, password lobbies) is specified in SPEC.md but not built yet.
