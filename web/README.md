# Orion web build

**It works.** The full game runs in the browser on WebGPU — menus,
single-player vs the bot, the map editor. Verified in Chrome at ~70 FPS.

Build + run:

    ./web/build.sh
    cd web && python3 serve.py     # then open http://localhost:8765

Needs a WebGPU browser (Chrome/Edge). The 6144px atlas exceeds the
WebGL2 floor, so there is no webgl fallback.

Web-specific behavior:
- Online play is stubbed ("desktop build only") — a web-sys WebSocket
  transport for the relay is the natural next step.
- Settings/replays/custom maps don't persist (no filesystem);
  localStorage adapters would fix that.
- Audio depends on browser autoplay policy — it starts after the first
  user gesture at best, silently stays off at worst (never fatal).

Porting notes (for the curious): the deterministic sim compiled with
ZERO changes. Everything else was platform plumbing — native
TLS/HTTP/WebSocket deps cfg'd out behind a same-shape stub, Instant/
SystemTime through web-time, process::id gated, winit driven via
spawn_app (run_app's exception-escape frees the handler's stack), an
async wgpu bootstrap adopted on the next event-loop tick, and a manual
canvas/surface size because winit-web's ResizeObserver left the
backing store at 1x1.
