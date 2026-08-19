# AGENTS.md

Architecture and domain context live in `CLAUDE.md` and `SPEC.md`; read those
first. This file only adds environment/operational notes.

## Cursor Cloud specific instructions

Two crates in one workspace: `orion-sim` (deterministic simulation, no gfx) and
`orion-client` (winit + wgpu renderer). Standard commands are in `README.md`
(Run/Test) and `CLAUDE.md` (Verification loops); prefer those. Notes below are
the non-obvious bits for this environment.

- **Toolchain must be recent stable.** The README says "Rust 1.80+", but that
  is outdated: a transitive dep (`wayland-protocols`) requires `edition2024`,
  so the workspace only builds on Cargo/Rust 1.85+ (CI uses
  `dtolnay/rust-toolchain@stable`). This environment's default toolchain is a
  recent stable and builds cleanly; don't pin an older toolchain.

- **Only build-time system dep is `libasound2-dev`** (ALSA, for `rodio`), as
  documented in the README. It is already installed in this environment.

- **Running the client needs a display + software GPU.** `orion-client` always
  opens a winit window and renders through wgpu, even in the headless capture
  modes (`--shot`, `--script`, `--smoke`, `--menu-shot`, `--mp-auto`). In this
  environment there is no real display or GPU, so run it under Xvfb with a
  software Vulkan device (Mesa lavapipe / `llvmpipe`, already installed) and a
  valid `XDG_RUNTIME_DIR`. Example:

  ```sh
  export XDG_RUNTIME_DIR=/tmp/xdg-runtime && mkdir -p "$XDG_RUNTIME_DIR" && chmod 700 "$XDG_RUNTIME_DIR"
  xvfb-run -a -s "-screen 0 1600x900x24" \
    cargo run --release -p orion-client -- --shot out.ppm --shot-ticks 6480
  ```

  Pure-sim work (`cargo test --workspace`, and the `orion-sim` examples like
  `soak`, `botgame`, `stress`, `balance`) needs none of this — it runs directly.

- **Client verification is headless, not manual playtesting.** Use `--shot`
  (one-frame capture of a bot game), `--script` (drives the real human command
  path — gather/build/multi-select/attack-move — and asserts a base got built),
  and `--mp-auto` (two-process lockstep smoke). Captures are PPM; convert with
  `pnmtopng` (netpbm, installed) since `sips` from the README is macOS-only.
