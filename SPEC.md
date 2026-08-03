# Orion — Specification

*A StarCraft-style RTS built from scratch in Rust. No game engine.*

Status: **draft spec, pre-implementation.** Derived from interview 2026-08-03.

---

## Problem Statement

StarCraft-style RTS is a genre with a devoted audience and almost no new supply. The
games that define it are 15–30 years old, and modern attempts either bury the formula
under a general-purpose engine that fights the genre's requirements (deterministic
simulation, hundreds of units, fixed-tick lockstep networking) or dilute the design
until it stops being an RTS. This project builds a real one: a 1v1-focused,
base-building, fog-of-war RTS with SC1's match shape, on a purpose-built Rust stack
where every layer is chosen for what the genre actually needs. The first version is
deliberately a faithful clone — the goal is to prove the machinery and the feel before
inventing new mechanics, and to leave the design surface open so a distinguishing twist
can be added once the game is playable enough to test one.

---

## Requirements

### Must Have

**Core loop**
- Resource harvesting → base building → tech progression → army production → combat
- Match shape: **15–25 minutes, ~200 supply cap**, SC1-like economy pacing
- One race at launch (mirror matchup), authored entirely in **data** so race #2 is
  content, not code
- One map at launch

**Unit control**
- Hundreds of units on screen, box select, control groups, no artificial selection cap
- Click-to-move that feels responsive: sub-frame command acknowledgement, no
  clumping/jitter under group movement
- Attack-move, hold position, patrol, stop

**Terrain**
- Multi-level terrain: **cliffs, ramps, high ground**
- High ground grants vision advantage (low ground cannot see up without detection)
- Terrain is **static at runtime** — no destructible/buildable terrain — so navigation
  data is fully precomputed per map

**Fog of war**
- Three states: unexplored / explored-but-not-visible / visible
- Per-player vision grid driven by unit and building sight radii
- Scouting is a real mechanic, not a cosmetic filter

**Presentation**
- **2D sprites, fixed isometric camera.** Pan and zoom only — the camera never rotates.
- Placeholder art is flat team-colored shapes; everything loads through the same
  sprite-atlas path as final art, so swapping art touches no code

**Multiplayer**
- **Deterministic lockstep** — peers exchange inputs only, both simulate identically
- **1v1 matchmaking queue** with MMR and a public ranked ladder
- **Private lobbies with a temporary password** for friends
- **Thin matchmaker + P2P play**: the server pairs players and brokers the handshake;
  the match itself runs peer-to-peer

**Single player**
- **Skirmish AI opponent** at multiple difficulties: real build orders, army
  composition, attack timing, and scouting — not a cheating stat-boosted bot

**Distribution**
- Native desktop binary (Windows / macOS / Linux). Store/platform decision deferred.

### Nice to Have

- **Replays.** Nearly free under lockstep (persist the input stream + map + seed) and
  the single most valuable desync-debugging tool that exists. Strongly recommended to
  build early as dev tooling even though it wasn't required as a feature.
- Second and third race, once race #1 is tuned and the data-driven content path is proven
- Observer / spectator mode
- Additional maps
- A distinguishing mechanic that separates Orion from a straight clone (deferred by
  design — see Open Questions)

### Explicitly Out of Scope

- **Any off-the-shelf game engine.** No Unity, no Unreal, no Godot. Also **not Bevy** —
  its scheduler and float-heavy transform stack actively fight bit-level determinism,
  and it is a large engine regardless of how it's marketed.
- **3D rendering and free camera.** Locked 2:1 isometric, permanently. This is a
  deliberate, accepted one-way door.
- **Map editor.** Maps are authored as data files by hand for now.
- **Campaign / story mode.**
- **Full authoritative server.** No server-side simulation.
- **Anti-cheat beyond desync detection.** Maphacks are known to be undetectable in this
  architecture and are accepted (see Trade-offs).
- **Destructible or player-modified terrain.**
- **Team games (2v2+).** 1v1 only.
- **Browser/WASM build.**

---

## Technical Approach

### Language and stack — Rust

Chosen over C/C++ for a project of this shape: no memory-corruption debugging on a
500-unit simulation, `cargo` instead of CMake dependency archaeology, first-class
cross-compilation to all three desktop targets, and a mature ecosystem for the parts
that aren't the interesting problem (serialization, networking, audio).

| Layer | Choice | Rationale |
|---|---|---|
| Window / input | `winit` | The standard, does one thing |
| Rendering | `wgpu`, custom instanced sprite batcher | Hand-rolled 2D quad renderer over a texture atlas. A full engine is unnecessary for a fixed-camera 2D game; one instanced draw call per atlas handles thousands of sprites. |
| Simulation | Hand-rolled, ECS-flavored | Dense arrays keyed by generational entity IDs. No third-party ECS — determinism requires full control over iteration order. |
| Content | RON / TOML data files | Units, buildings, tech, costs, timings. This is what makes "1 race now, N later" true. |
| Networking | `tokio` + UDP, custom lockstep protocol | Inputs only; a handful of bytes per tick. |
| Matchmaker | Rust service (`axum`) + SQLite | Accounts, MMR, queue, lobby brokering. Deliberately small enough to run on one cheap VPS. |

### Determinism is the foundational constraint

Lockstep means every peer must produce a **bit-identical** simulation from the same
inputs. This is not a feature that can be added later — retrofitting it is a rewrite.
It is therefore designed in from the first commit:

- **Fixed-point arithmetic throughout the sim.** Q16.16 (`i32`/`i64`) for positions,
  velocities, health, timers. Custom `Fixed`, `FixedVec2` types with deterministic
  `sqrt`, `atan2`, and trig via lookup tables.
- **No floating point in the simulation layer, ever.** Floats exist only in the
  renderer, where divergence is harmless.
- **No `HashMap` iteration** in sim code — hash ordering varies. Sorted `Vec` or
  `BTreeMap` only.
- **Deterministic RNG**: seeded xorshift, seed exchanged at match start, advanced only
  from sim code.
- **Hard architectural split** between `sim` and `render` crates, enforced by the module
  boundary. `sim` has no dependency that can introduce nondeterminism.
- **Continuous verification**: state checksum every tick in dev builds, compared between
  peers. A desync fails loudly at the tick it happened, not ten minutes later. Combined
  with replay capture, this makes the hardest bug class in the project tractable.

### Simulation and rendering cadence

- Sim runs at a **fixed 24 Hz tick** (SC1's "fastest" speed, and a natural fit for
  sprite animation frame rates).
- Render runs at display refresh, **interpolating** between the previous and current sim
  state. Decoupling these is what makes 24 Hz feel smooth.
- Input delay of ~3 ticks (~125 ms) hides network latency; commands issued on tick N
  execute on tick N+3. Genre-standard, and the reason RTS feels fine at latencies that
  would ruin a shooter.

### Pathfinding — the primary technical risk

Hundreds of units navigating multi-level terrain with ramps, without clumping or
deadlocking, is the hardest engineering problem in the project. Approach:

1. **Precomputed per map** — terrain is static, so all navigation structure is baked at
   map load: passability grid, cliff/ramp connectivity, and a sector+portal graph for
   long-range queries.
2. **Hierarchical long-range**: A* over the sector/portal graph, not over raw tiles.
3. **Flow fields for group movement**: one field computed per move order, shared by
   every unit in that order. Cost is per-order, not per-unit — this is what makes 200
   units affordable.
4. **Deterministic local avoidance**: integer-based separation/push resolution in a
   fixed iteration order. Not RVO — RVO is float-heavy and order-sensitive.

Unit collision is soft (units push each other) rather than hard, matching SC1's feel and
avoiding the deadlock failure mode of rigid collision.

### Fog of war

Per-player visibility grid at sim resolution, updated incrementally — units stamp
precomputed circular vision masks as they move rather than recomputing radii each tick.
Three-state (unexplored / explored / visible) with the explored layer persisted per
player. High-ground vision rules read directly from the terrain height layer.

Fog is **rendered** client-side but **enforced** in the sim: the sim decides what a
player's entity list can reference so that targeting and command validation respect
vision. This does not stop a maphack (an attacker can always modify their own render
layer) but it prevents fog bugs from becoming gameplay bugs.

### Art pipeline

Final art will be **AI-generated sprite sheets**. The known weakness is consistency —
holding a single unit's identity across 16 facings and multiple animation states is
genuinely hard for image models, and incoherent frames read as flicker in motion.

The insurance is that **the runtime never knows where sprites came from**. The engine
consumes a documented atlas format (packed texture + JSON manifest: unit → animation →
facing → frame rects, anchor points, team-color mask channel). An importer tool produces
that format from any source. If AI generation fails the consistency bar, the fallback is
the technique SC1, C&C, and Diablo actually used — model once in Blender, script-render
16 angles × all animations to the same atlas format — and **not a single line of engine
code changes.** Placeholder shapes ship through the identical path from day one, so the
pipeline is exercised continuously rather than discovered at the end.

### Networking topology

```
                    ┌──────────────────┐
                    │   Matchmaker     │  accounts, MMR, ladder,
                    │  (Rust + SQLite) │  queue, password lobbies
                    └────────┬─────────┘
                       pair + handshake
                    ┌────────┴─────────┐
                    ▼                  ▼
              ┌──────────┐  lockstep  ┌──────────┐
              │ Client A │◄──inputs──►│ Client B │
              └──────────┘    (P2P)   └──────────┘
```

The matchmaker pairs players, brokers the connection, then steps out of the match. It
also **doubles as a fallback relay**: pure P2P fails behind symmetric NAT for a
meaningful fraction of players, and "the queue found you a game but you can't connect"
is an unacceptable failure. Relay mode costs one extra hop of latency and is only used
when hole-punching fails.

The AI opponent is implemented as **a local input source feeding the same command
stream** as a human player. This means the bot is deterministic, replay-compatible, and
testable by replaying its games — rather than a special case threaded through the sim.

---

## Trade-offs Accepted

- **Fixed camera is permanent.** 2D sprites with a locked isometric angle rules out
  camera rotation forever. Accepted knowingly in exchange for a far simpler renderer and
  faster time-to-playable.
- **Maphacks are possible and will not be prevented.** In P2P lockstep, fog is drawn
  client-side; a modified client can simply not draw it, and nothing about that diverges
  the simulation. Desync detection catches resource and stat hacks but is structurally
  blind to this one. The ladder is therefore explicitly **casual** — MMR exists to
  produce good matches, not to confer prestige.
- **One race means mirror-only matchups**, which will hide entire categories of balance
  problem until race #2 exists. The mitigation is that this is *known*, not that it's
  solved.
- **Determinism taxes every line of sim code.** No convenient float math, no ergonomic
  hash maps, constant discipline. This is the price of RTS-scale netcode and it is paid
  continuously, not once.
- **"Build systems, then play"** means feel-level problems surface late. Explicitly
  chosen; the counterweight is getting to a playable vertical slice as the first
  integration milestone rather than a late one.

---

## Open Questions / Risks

**Design**
1. **What is Orion actually about?** The decision was "faithful clone now, find the
   twist by playing." That is a legitimate sequencing choice, but it leaves the central
   product risk unresolved: a competent StarCraft clone competes with StarCraft, which
   is free. A distinguishing mechanic needs to be found and it needs to be found before
   the ladder goes live, not after.
2. **Race identity and theme** — undecided. Affects unit roster, silhouettes, and every
   sprite generated.
3. **Unit roster for race #1** — how many units, what the counter triangle is, what the
   signature mechanic is. Needs a design pass before content authoring starts.
4. **Map layout** — one map, but 1v1 with how many bases, what ramp topology, what
   expansion pacing? Map design *is* balance design in a mirror matchup.

**Technical**
5. **AI sprite consistency across 16 facings is unproven.** This should be tested early
   with a single unit end-to-end before committing to the whole roster. The Blender
   fallback exists but represents a real change in effort profile.
6. **NAT traversal success rate is unknown** until measured with real players on real
   networks. Relay fallback capacity is a hosting cost that can't be estimated yet.
7. **Pathfinding at 200 units on multi-level terrain** — the flow-field + hierarchical
   approach is sound, but the deterministic local-avoidance step is where similar
   projects tend to fail. Needs a standalone stress test (200 units, cliffs, ramps,
   chokepoints) before the rest of the game depends on it.
8. **Skirmish AI is a much larger project than it looks.** A bot with real build orders,
   composition logic, timing attacks, and scouting is arguably a second game's worth of
   design work, and it is on the must-have list.

**Product**
9. **Distribution deferred.** Steam would solve NAT traversal, friends, and updates
   outright — deciding it later means potentially building infrastructure that Steam
   would have provided for $100.
10. **A ranked ladder needs a player base to mean anything.** With too few players, MMR
    produces bad matches regardless of correctness. No plan yet for how players are found.

---

## Recommended First Milestone

Not part of the answered scope, but flagged: before the vertical slice, the two items
worth de-risking standalone are **(a)** 200 units pathing across cliffs and ramps at
60 fps with no clumping, and **(b)** one unit taken end-to-end through the AI sprite
pipeline at 16 facings. Both are cheap to test, both are load-bearing for everything
else, and both are much more expensive to discover broken later.
