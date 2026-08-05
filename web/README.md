# Orion web build (experimental)

Status: compiles and boots in-browser — canvas attaches, the WebGPU
adapter and device initialize, the full procedural atlas paints
(console shows `orion: init complete`) — but frames don't reach the
canvas yet. Black screen.

Build:

    rustup target add wasm32-unknown-unknown
    cargo build --release -p orion-client --target wasm32-unknown-unknown
    wasm-bindgen --target web --out-dir web/dist --no-typescript \
        target/wasm32-unknown-unknown/release/orion-client.wasm
    (cd web && python3 -m http.server 8765)  # open localhost:8765

Needs a WebGPU browser (Chrome/Edge); the 6144px atlas exceeds the
WebGL2 floor, so no webgl fallback.

What's stubbed on web: online play (relay_wasm.rs answers every entry
point with "desktop build only" through the same channel shapes),
native TLS/HTTP deps are cfg'd out, `std::time::Instant` goes through
web-time.

Next debugging steps (in order of suspicion):
1. Surface/canvas size negotiation — request_inner_size is issued, but
   confirm winit's Resized event actually fires and reaches
   App::resize; log inner_size at Gfx::new_async.
2. Confirm RedrawRequested fires after init (the pre-init pump works;
   post-init frames may not be scheduled — try requesting a redraw
   directly in finish_init).
3. Surface format: web prefers Bgra8Unorm; check what
   surface.get_default_config picked and whether our sRGB conversion
   assumptions hold.
