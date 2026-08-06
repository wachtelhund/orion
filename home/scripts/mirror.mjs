// Copies everything the homepage needs out of the game's repo (which may be
// private) into this directory tree, so the deployed site is fully
// self-contained and visitors never touch GitHub at all:
//
//   public/mirror/README.md       copy for the site's wording
//   public/mirror/repo.json       license, language, visibility
//   public/mirror/changelog/*.md  wiki pages + images/ for their screenshots
//   public/mirror/releases.json   the download manifest (GitHub API shape)
//   public/downloads/<tag>/       the compiled binaries, served as assets
//   public/play/dist/             the browser build from the featured release
//
// Reads the source repo with $ORION_TOKEN when set (required once it is
// private); falls back to your `gh` login, then to unauthenticated access.
//
// The game's own repo is never modified.

import { execFileSync } from 'node:child_process';
import { cp, mkdir, mkdtemp, readFile, readdir, rm, stat, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import { DOWNLOADS_BASE, DOWNLOADS_DIR, MIRROR_DIR, SOURCE_REPO } from '../public/config.js';
import { compareTags, isComplete } from '../public/versions.js';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const MIRROR = join(root, MIRROR_DIR);
const CHANGELOG = join(MIRROR, 'changelog');
const DOWNLOADS = join(root, DOWNLOADS_DIR);
const PLAY_DIST = join(root, 'public', 'play', 'dist');
const WEB_ASSET = 'orion-web-demo.zip';

const SOURCE = `${SOURCE_REPO.owner}/${SOURCE_REPO.repo}`;

/**
 * Credentials for reading the source repo, in preference order:
 *
 *  1. $ORION_TOKEN — only needed in CI, which has no interactive login.
 *  2. Your existing `gh` login. If you can browse the repo on your own
 *     machine, the mirror can read it, so running this locally needs no
 *     token, no secret and no setup.
 */
function sourceCredentials() {
  if (process.env.ORION_TOKEN) return { token: process.env.ORION_TOKEN, from: 'ORION_TOKEN' };
  try {
    const token = execFileSync('gh', ['auth', 'token'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'ignore'],
    }).trim();
    if (token) return { token, from: 'your gh login' };
  } catch {
    /* not logged in — fall through to anonymous */
  }
  return { token: '', from: 'anonymous' };
}

const { token: sourceToken, from: sourceAuth } = sourceCredentials();

/** Run gh, optionally as a different identity than the ambient login. */
function gh(args, token) {
  return execFileSync('gh', args, {
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
    env: token ? { ...process.env, GH_TOKEN: token } : process.env,
  });
}

function git(args, cwd = root) {
  return execFileSync('git', args, { cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
}

const ghJson = (path, token) => JSON.parse(gh(['api', path], token));

/**
 * Wikis are a separate git repo and are not served by the raw CDN for private
 * projects, so clone rather than fetch. The token goes in the URL, which is why
 * the clone happens in a temp dir that never lands in this repo.
 */
async function mirrorWiki() {
  const auth = sourceToken ? `x-access-token:${sourceToken}@` : '';
  const url = `https://${auth}github.com/${SOURCE}.wiki.git`;
  const temp = await mkdtemp(join(tmpdir(), 'orion-wiki-'));

  try {
    git(['clone', '--depth', '1', '--quiet', url, temp], tmpdir());
  } catch (error) {
    // Never let a token reach the logs via git's error output.
    throw new Error(
      `wiki clone failed (exit ${error.status}) reading as ${sourceAuth}. ` +
        'A private wiki needs the `repo` scope — classic PAT, not fine-grained.',
    );
  }

  await rm(CHANGELOG, { recursive: true, force: true });
  await mkdir(CHANGELOG, { recursive: true });
  for (const entry of await readdir(temp, { withFileTypes: true })) {
    if (entry.name === '.git') continue;
    await cp(join(temp, entry.name), join(CHANGELOG, entry.name), { recursive: true });
  }
  await rm(temp, { recursive: true, force: true });

  const pages = (await readdir(CHANGELOG)).filter((f) => f.endsWith('.md'));
  console.log(`  wiki       ${pages.length} pages`);
}

/** Is the game's repo readable without credentials right now? */
async function probeSourceIsPublic() {
  const res = await fetch(`https://api.github.com/repos/${SOURCE}`, {
    headers: { 'user-agent': 'orion-home-mirror' },
  });
  return res.ok;
}

async function mirrorRepoContent() {
  const repo = ghJson(`repos/${SOURCE}`, sourceToken);
  const readme = Buffer.from(
    ghJson(`repos/${SOURCE}/readme`, sourceToken).content,
    'base64',
  ).toString('utf8');

  await writeFile(join(MIRROR, 'README.md'), readme);
  await writeFile(
    join(MIRROR, 'repo.json'),
    `${JSON.stringify(
      {
        name: repo.name,
        fullName: repo.full_name,
        url: repo.html_url,
        description: repo.description || '',
        language: repo.language,
        license: repo.license?.spdx_id || '',
        sourceIsPublic: await probeSourceIsPublic(),
      },
      null,
      2,
    )}\n`,
  );
  console.log(`  readme     ${readme.length} bytes`);
}

/** How many recent releases to keep available publicly. */
const KEEP_RELEASES = 3;

/**
 * Download one release's binaries into public/downloads/<tag>/ and return a
 * GitHub-API-shaped release object whose asset URLs are same-origin paths —
 * versions.js and build.mjs consume it exactly like the live API's answer.
 */
async function mirrorOne(release) {
  const tag = release.tag_name;
  const dir = join(DOWNLOADS, tag);
  await rm(dir, { recursive: true, force: true });
  await mkdir(dir, { recursive: true });
  gh(['release', 'download', tag, '--repo', SOURCE, '--dir', dir], sourceToken);
  // The browser build ships separately at /play — not a download button.
  await rm(join(dir, WEB_ASSET), { force: true });

  const assets = [];
  for (const name of (await readdir(dir)).sort()) {
    const info = await stat(join(dir, name));
    assets.push({
      name,
      size: info.size,
      browser_download_url: `${DOWNLOADS_BASE}/${tag}/${name}`,
    });
  }
  console.log(`  release    ${tag} mirrored with ${assets.length} assets`);
  return {
    tag_name: tag,
    name: release.name || tag,
    html_url: '#download',
    draft: false,
    prerelease: false,
    published_at: release.published_at,
    assets,
  };
}

/**
 * Pull recent builds into the site's own asset tree so the download buttons
 * are same-origin — no public GitHub repo on the visitor path. Tags outside
 * the keep-window are pruned so the deploy never grows without bound.
 */
async function mirrorReleases() {
  const source = ghJson(`repos/${SOURCE}/releases?per_page=100`, sourceToken)
    .filter((r) => !r.draft && !r.prerelease)
    .sort((a, b) => compareTags(a.tag_name, b.tag_name))
    .slice(0, KEEP_RELEASES)
    .reverse();

  const keep = new Set(source.map((r) => r.tag_name));
  if (existsSync(DOWNLOADS)) {
    for (const entry of await readdir(DOWNLOADS)) {
      if (!keep.has(entry)) {
        await rm(join(DOWNLOADS, entry), { recursive: true, force: true });
        console.log(`  release    ${entry} pruned (outside keep-window)`);
      }
    }
  }

  const manifest = [];
  for (const release of source) manifest.push(await mirrorOne(release));
  await writeFile(
    join(MIRROR, 'releases.json'),
    `${JSON.stringify(manifest, null, 2)}\n`,
  );

  // The web build must match whatever the site features, and the site features
  // the newest COMPLETE release (see versions.js) so every platform and the
  // browser stay on one PROTOCOL_VERSION. `source` is oldest-first here.
  const featured = [...source].reverse().find(isComplete) ?? source[source.length - 1];
  return featured ? featured.tag_name : '';
}

/**
 * The version compiled into a wasm bundle, read from the binary itself.
 *
 * The game stamps CARGO_PKG_VERSION on its main menu as `V1.2.3`, so the string
 * is in the data section. This is the only trustworthy source: the repo's
 * committed `web/dist` has been a release behind at every tag checked, so
 * anything derived from commit metadata reports the wrong version.
 */
function wasmVersion(buffer) {
  const found = new Set(
    [...buffer.toString('latin1').matchAll(/V(\d+\.\d+\.\d+)/g)].map((m) => m[1]),
  );
  if (found.size !== 1) {
    throw new Error(
      `could not read a single version stamp from the wasm (found ${found.size})`,
    );
  }
  return [...found][0];
}

/**
 * The browser build, taken from the **release's own `orion-web-demo.zip`** —
 * not from `web/dist` in the repo tree, which lags a release behind.
 *
 * This matters beyond tidiness: PROTOCOL_VERSION is the crate version, so a
 * browser bundle from a different release refuses to play with desktop peers.
 * Taking it from the same release the download buttons serve makes them the
 * same build by construction, and the version stamped inside the wasm is
 * checked against the tag before anything is published.
 */
async function mirrorWebBuild(tag) {
  if (!tag) {
    console.log('  web build  skipped (no release to match)');
    return false;
  }

  const temp = await mkdtemp(join(tmpdir(), 'orion-web-'));
  try {
    gh(
      ['release', 'download', tag, '--repo', SOURCE, '--pattern', WEB_ASSET, '--dir', temp],
      sourceToken,
    );
  } catch {
    console.log(`  web build  ${tag} has no ${WEB_ASSET} — /play left off`);
    await rm(temp, { recursive: true, force: true });
    return false;
  }

  execFileSync('unzip', ['-oq', join(temp, WEB_ASSET), '-d', temp]);
  const wasm = await readFile(join(temp, 'dist', 'orion-client_bg.wasm'));
  const built = wasmVersion(wasm);

  if (`v${built}` !== tag) {
    await rm(temp, { recursive: true, force: true });
    throw new Error(
      `${WEB_ASSET} on ${tag} contains a v${built} build. PROTOCOL_VERSION is ` +
        'the crate version, so browser players could not join desktop games. ' +
        'Rebuild the web bundle from the tag and re-upload it.',
    );
  }

  await mkdir(PLAY_DIST, { recursive: true });
  const files = ['orion-client.js', 'orion-client_bg.wasm'];
  for (const name of files) {
    await cp(join(temp, 'dist', name), join(PLAY_DIST, name));
  }
  await rm(temp, { recursive: true, force: true });

  await writeFile(
    join(MIRROR, 'web.json'),
    `${JSON.stringify(
      { version: tag, wasmBytes: wasm.length, source: WEB_ASSET, files },
      null,
      2,
    )}\n`,
  );
  console.log(
    `  web build  ${tag} (${(wasm.length / 1024 / 1024).toFixed(1)} MB wasm, ` +
      `stamped V${built}) — matches downloads`,
  );
  return true;
}

async function main() {
  console.log(`Mirroring ${SOURCE} -> site assets  (reading as: ${sourceAuth})`);
  if (!sourceToken) {
    console.log('  ! anonymous — this only works while the game repo is public.');
    console.log('    Run `gh auth login`, or set ORION_TOKEN in CI.');
  }
  await mkdir(MIRROR, { recursive: true });

  await mirrorRepoContent();
  await mirrorWiki();
  const newestTag = await mirrorReleases();
  await mirrorWebBuild(newestTag);

  console.log(`Mirror written to ${MIRROR}`);
}

if (!existsSync(join(root, 'package.json'))) throw new Error('run from the repo root');
await main();
