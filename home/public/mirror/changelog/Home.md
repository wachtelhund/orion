# Orion

A StarCraft-style RTS built from scratch in Rust — deterministic lockstep
sim, procedural pixel art and audio, two asymmetric races, ranked online
multiplayer. Grab a build from
[Releases](https://github.com/wachtelhund/orion/releases).

## Version history

Every minor/major release gets a page here: what changed, why, and how it
looks in game.

| Version | Highlights |
|---|---|
| [[v0.20.0]] | Replay links: ?replay=CODE plays a shared game from a URL; v0.20.1: browser brightness/sound/disconnect fixes from the first real web playtest |
| [[v0.19.0]] | Browser build lists and joins public lobbies (CORS + wasm fetch); headless-Chrome page hooks for web UI verification |
| [[v0.18.0]] | Replay sharing by code: upload a replay to the relay, hand out a 5-letter code, it lands in the other player's list |
| [[v0.17.0]] | Causeway: 4th map around a contested land bridge with breachable rock ramps; live map-preview thumbnails in the SP/MP pickers; v0.17.1: demand-driven bot gas, bots research upgrades, summon spawn safety |
| [[v0.16.0]] | Browser multiplayer: code lobbies + cross-play with desktop through the live relay, localStorage settings, hidden-tab keep-alive |
| [[v0.15.0]] | Smooth unit turning, idle-worker alert chip, per-ability cast sounds; v0.15.1: THE GAME RUNS IN THE BROWSER (WebGPU, ~70 FPS) |
| [[v0.14.0]] | Visual map editor with auto-mirrored painting, custom maps in the SP picker |
| [[v0.13.0]] | Heroes: Marshal Kade, Broodmother Sszrak, Magnus Vex — two abilities each, bot support, deterministic zones/summons |
| [[v0.12.0]] | AI ability micro + kiting + mop-up sweep (Thornwood stall fixed), Ferron mass rebuild, MP connection readout |
| [[v0.11.1]] | MP lag fix (adaptive input delay, stall-flush, nodelay), 3D shield dome, console/menu layout fixes |
| [[v0.11.0]] | The juice pass: additive glow lighting, per-weapon combat effects + sounds, material-true deaths, drifting fog of war, shadows, ambient emitters |
| [[v0.10.0]] | The Ferron Compact (third race), Bulwark + Burrower abilities, bust portraits, audio overhaul, deep forests, HUD/menu fixes |
| [[v0.9.0]] | The graphics overhaul: full 4x repaint — dystopian terrain, hard-edged angular units, industrial/organic bases, new forests, minerals and effects |
| [[v0.8.0]] | Thornwood: destructible forests with line-of-sight occlusion, high-ground third bases, observer battle GIF |
| [[v0.7.0]] | Combat-sim AI (the army ball, measured via A/B arena), version-safe MP + update prompt, match-start countdown |
| [[v0.6.0]] | AI overhaul: perception, memory, threat response, counter-production, strength-aware attack posture |
| [[v0.5.0]] | In-game ladder (top 25 by Elo), cross-race balance pass, professional polish sweep of every screen |
| [[v0.4.0]] | SC:R-style console + menu rework (gold/navy chrome), building box-select, update notice, overlap-free HUD |
| [[v0.3.0]] | Ranked matchmaking (MMR + latency), replays, second map "Caverns" with expansions, bot siege/storm/scout, spawn-fairness overhaul, storm nerf |
| [[v0.2.0]] | IP-free multiplayer (lobby browser + private codes), double-click releases for all platforms, multi-worker build queueing, worker visual feedback |
| [[v0.1.0]] | First playable release: two races, full macro loop, skirmish AI, online MP via relay codes |
