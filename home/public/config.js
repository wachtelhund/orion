// Where the site gets its content. Shared by the build (Node), the browser
// runtime, and the mirror script so they can never drift apart.
//
// The game's repo may be private, and nothing a visitor's browser touches is
// allowed to resolve there. The site is therefore fully self-contained:
// `scripts/mirror.mjs` copies everything it needs — README, wiki pages,
// release binaries, the browser build — into this directory tree, and it all
// deploys together as static assets. No GitHub repo is on the visitor path.

/** The game. Source of truth; may be private. */
export const SOURCE_REPO = { owner: 'wachtelhund', repo: 'orion' };

/** Directory holding mirrored site content (served at /mirror). */
export const MIRROR_DIR = 'public/mirror';

/** Directory holding mirrored release binaries (served at /downloads). */
export const DOWNLOADS_DIR = 'public/downloads';

/** Same-origin base for mirrored content — changelog pages and their images. */
export const MIRROR_RAW = '/mirror';

export const CHANGELOG_RAW = `${MIRROR_RAW}/changelog`;

/** Same-origin base the download buttons point at. */
export const DOWNLOADS_BASE = '/downloads';

export const SOURCE_REPO_URL = `https://github.com/${SOURCE_REPO.owner}/${SOURCE_REPO.repo}`;
