# orion-home

The homepage for [Orion](https://github.com/wachtelhund/orion) — a static site
at **[orion.hampusnilsson.dev](https://orion.hampusnilsson.dev)** where people
can read about the game, download the latest build, and browse the changelog.

It is a landing page, not a documentation dump: a full-screen hero over
gameplay footage, one short paragraph, download buttons, and a collapsed
changelog. There is deliberately no screenshot gallery — stills of a game in
active development go stale fast, and the hero clip carries the visuals.

Nothing on the page is typed by hand — every string comes from the Orion repo:

| On the page | Source |
|---|---|
| Hero line + the four pills under it | wiki `Home.md` lede, split on its em dash |
| Short description | `README.md` lede (minus the sentence the hero already says) |
| Download cards (steps per OS) | `README.md` → `## Install` table |
| Version, dates, file names and sizes | GitHub Releases API |
| Changelog | GitHub wiki (`Home.md` version table + one page per version) |

To change the site's copy, edit the Orion repo or its wiki — then rebuild.

## The game's repo is private — the site doesn't care

This directory lives INSIDE the game's repo (`home/`), and the deployed site
is **fully self-contained**: every URL a visitor touches — downloads,
changelog text, changelog images, the browser build — is a same-origin static
asset on Cloudflare. No GitHub repo, token or API is on the visitor path.

**After cutting a new Orion release, run one command from `home/`:**

```sh
npm run publish     # mirror -> build -> test -> deploy
```

That needs **no token and no setup** locally. It reads the repo through your
existing `gh` login — if you can browse the repo, the mirror can read it.
There is also a manual GitHub Action (`Deploy homepage` in the Actions tab)
that does the same; it only needs the `CLOUDFLARE_API_TOKEN` repo secret.

`npm run mirror` copies what the site needs into the asset tree:

| Mirrored | Into |
|---|---|
| `README.md` and repo metadata | `public/mirror/README.md`, `public/mirror/repo.json` |
| Wiki pages + their screenshots | `public/mirror/changelog/` |
| The 3 newest releases' binaries | `public/downloads/<tag>/` + `public/mirror/releases.json` |
| The browser build (`orion-web-demo.zip`) | `public/play/dist/` — served at `/play` |

Nothing needs configuring when visibility changes: `mirror.mjs` probes whether
the game repo is publicly readable and records it in `repo.json`, and the build
swaps every source link for an on-page link on its own — "View source" becomes
"All downloads".

## Play in the browser

`/play` runs the real game on WebGPU — no install, Chrome or Edge only (the
6144px atlas is past the WebGL2 ceiling, so there is no fallback).

Two rules keep it honest, both in `scripts/mirror.mjs`:

- The bundle comes from the **release's own `orion-web-demo.zip`**, never from
  `web/dist` in the repo tree. That directory has been a release behind at every
  tag checked, so it cannot be trusted.
- The version stamped **inside the wasm** is checked against the release tag
  before publishing, and a mismatch aborts the mirror. This is not pedantry:
  `PROTOCOL_VERSION` is the crate version, so a bundle from another release
  cannot join a game.

If the featured release has no web bundle, `/play` is simply left off — the
button and nav link disappear rather than pointing at a broken page.

## How it stays current

Two layers, so the page is both fast and fresh:

1. **Build time** — `npm run build` fetches GitHub and writes a complete
   `public/index.html` plus `public/data/site.json`. The page needs no
   JavaScript to render, which keeps it fast and indexable.
2. **Runtime** — `public/app.js` re-checks the Releases API and the wiki in the
   visitor's browser. A release cut or wiki page written *after* the last deploy
   shows up without redeploying. If GitHub is unreachable or rate-limited, the
   build-time content simply stays, and it is always complete.

Both layers render Markdown with the same module (`public/md.js`), so the
build and the browser can't drift apart.

## Run

```sh
npm install
npm run mirror    # pull Orion's README, wiki and latest build into mirror/
npm run build     # mirror/ -> public/index.html + public/data/site.json
npm test          # unit tests for the Markdown renderer and wiki parsers
npm run dev       # serve public/ locally via wrangler (supports range requests,
                  # which python -m http.server does not — video needs them)
```

## Deploy

Cloudflare Workers static assets — free tier, no Worker script, so requests are
served straight off the CDN and nothing is billed per-request.

```sh
npm run deploy    # = npm run build && wrangler deploy
```

`wrangler.jsonc` declares the custom domain `orion.hampusnilsson.dev`; wrangler
manages the DNS record on the existing `hampusnilsson.dev` zone. Deploying needs
a Cloudflare login (`wrangler login`) — no secrets are stored in this repo.

## Media

`public/media/` holds only the hero clip: `orion.mp4` / `orion.webm`, plus
`hero-poster.jpg` (its own first frame, so nothing jumps when the video takes
over). The clip is upscaled 2x from `docs/media/orion.gif` with
**nearest-neighbour** scaling and rendered with `image-rendering: pixelated`, so
it stays crisp full-bleed instead of turning into a blurry smear.

Changelog screenshots are *not* here — those come from the wiki via
`mirror/changelog/images/`, are tied to the version that shipped them, and stay
accurate. Only the marketing gallery was dropped.

When the GIF is replaced upstream, re-run the commands below — the dimensions
are `2 x` whatever the new GIF is (`ffprobe` it).

```sh
# how the current hero clip was produced (GIF is 640x400 -> 2x = 1280x800)
ffmpeg -i orion.gif -movflags +faststart -pix_fmt yuv420p \
       -vf scale=1280:800:flags=neighbor -crf 28 -preset veryslow -an \
       public/media/orion.mp4
ffmpeg -i orion.gif -c:v libvpx-vp9 -crf 36 -b:v 0 -pix_fmt yuv420p \
       -vf scale=1280:800:flags=neighbor -an public/media/orion.webm
ffmpeg -i public/media/orion.mp4 -vframes 1 -q:v 4 public/media/hero-poster.jpg
```
