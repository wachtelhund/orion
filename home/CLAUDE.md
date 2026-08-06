# CLAUDE.md — orion-home

Static homepage for the Orion RTS, deployed to `orion.hampusnilsson.dev` on
Cloudflare Workers static assets (free tier).

## Architecture

```
public/config.js          WHERE content comes from — shared by build + browser
scripts/mirror.mjs        private game repo -> mirror/ + a release on THIS repo
src/index.template.html   page skeleton with {{slot}} placeholders
scripts/build.mjs         mirror/ -> renders public/index.html + data/site.json
public/md.js              Markdown subset renderer (shared by build + browser)
public/app.js             runtime refresh against the live GitHub API and wiki
public/styles.css         hand-written CSS, no framework, no build step
public/media/             hero clip (mp4/webm) + its poster frame
public/play/              browser build: page + dist/*.wasm (mirrored, gitignored)
src/play.template.html    /play page skeleton
public/data/site.json     build-time snapshot (also the browser's fallback)
test/md.test.mjs          node:test unit tests for md.js
```

No bundler, no framework, no runtime dependencies. `wrangler` is the only
devDependency and exists purely to deploy.

## The mirror boundary

The game's repo (`SOURCE_REPO`) may be private. **Nothing a visitor's browser
requests may resolve at GitHub at all** — the deployed site is fully
self-contained static assets: downloads at `/downloads/<tag>/`, changelog at
`/mirror/changelog/`, the browser build at `/play/`. `scripts/mirror.mjs`
copies everything in; `public/config.js` is the single place that says where
each thing lives.

If you add anything that fetches, check where it lands:

- Browser (`app.js`, `md.js`) → same-origin paths only (`CHANGELOG_RAW`,
  `DOWNLOADS_BASE`). Never a token, never GitHub.
- Build (`build.mjs`) → the local `public/mirror/` directory only. No network
  calls, no credentials.
- Mirror (`mirror.mjs`) → the only thing allowed to read the source repo,
  using `ORION_TOKEN` (or your local `gh` login). It clones the wiki over git
  because the raw CDN does not serve private wikis, so the token lands in a
  URL — keep that clone in a temp dir and keep git's stderr out of the logs.

`repo.json.sourceIsPublic` is probed, not configured: `mirror.mjs` makes an
unauthenticated request to the source repo and records whether it answered. The
build reads that to decide whether "View source" points at real source or turns
into "All downloads". Verify both paths after touching link rendering — flip the
flag in `mirror/repo.json`, rebuild, and grep the HTML for source-repo links.

## Versions must agree everywhere

`PROTOCOL_VERSION` is the game's crate version, so the desktop downloads and the
browser build have to be the *same release* or players cannot join each other.
Three things enforce that, and all three earned their place:

1. `featuredRelease()` picks the newest release that has **all three** desktop
   builds. v0.20.0 shipped macOS-only (CI minutes ran out), and featuring it
   would have left Windows and Linux with no download at all.
2. The web bundle is taken from that same release's `orion-web-demo.zip` —
   **not** `web/dist` in the repo tree, which was a release behind at v0.18.0,
   v0.19.0 and v0.20.0.
3. `wasmVersion()` reads the version stamped inside the wasm and refuses to
   publish if it disagrees with the tag. Commit metadata is not good enough:
   deriving the version from "the commit that last touched web/dist" reported
   v0.19.0 for a bundle that was actually v0.18.0.

Verify by loading `/play` and reading the version stamp at the bottom-right of
the main menu — it must equal the download button's version.

## Two rules that pull against each other

**1. It's a landing page, not documentation.** An earlier version rendered the
README's whole `## Status` bullet list as 22 feature cards; it read like a spec
sheet and was rejected. Keep it: full-bleed hero over gameplay, one short
paragraph, download buttons, collapsed changelog. No screenshot gallery: it
was removed because stills of a game in development go stale fast. If you're
adding a section, ask whether a visitor deciding to download needs it.

**2. No string is typed by hand.** Everything still comes from GitHub:

- **repo metadata** — license, language, URLs (`/repos/wachtelhund/orion`)
- **wiki `Home.md` lede** — split on its em dash into the hero line and the
  four pills. The wiki's own one-liner is the best marketing copy the project
  has; `heroCopy()` warns and falls back to the README if that shape changes.
- **README.md** — lede (short description, with the hero's sentence removed by
  `introAfterHero()` so they don't print back to back), `## Install` table
  (per-OS steps)
- **Releases API** — tag, publish date, asset names/sizes/URLs
- **Wiki** — `Home.md` version table, then one page per version (changelog)

Getting more marketing polish means finding a better *source* sentence, or
parsing it more cleverly — not typing copy into the template. Slots with no
value throw at build time rather than rendering blank.

## Two-layer freshness

`build.mjs` writes fully-rendered HTML (fast, no-JS, indexable). `app.js` then
re-fetches the live release and wiki in the browser and patches the DOM if
GitHub has moved ahead of the last deploy. Consequences worth remembering:

- Both layers must render Markdown identically → they import the same `md.js`.
- Anything `app.js` patches needs a stable id/`data-` hook in the template
  (`#hero-eyebrow`, `#release-tag`, `#downloads .dl[data-platform]`).
- GitHub calls from the browser are unauthenticated (60/h per visitor IP).
  Every fetch is inside `Promise.allSettled` — a failure must be a no-op, never
  a broken page.

## md.js

A deliberate Markdown *subset* — exactly what the Orion wiki and README use.
Notes for anyone changing it:

- Input is escaped **first**, then markup is applied. Code spans are stashed
  behind NUL sentinels so their contents are never parsed as markup. Wiki text
  is untrusted-ish input; keep it that way — never inject a raw wiki string as
  HTML without going through the renderer or `textContent`.
- List items accumulate **raw** text and render inline markup once at flush
  time. This matters: the wiki wraps `**bold spans**` across source lines, and
  rendering per-line leaves them unclosed.
- README links are repo-relative and wiki links are wiki-relative — pass the
  right `resolveUrl` (`renderReadme()` vs. the default) or links 404.

Every one of these behaviours has a test. Run `npm test` before deploying.

## Cost

Must stay free: static assets only (no `main` in `wrangler.jsonc`, so no Worker
invocations), no KV/D1/R2, no Cloudflare Images, observability off. Media is
optimised locally and committed; nothing transforms images at request time.
