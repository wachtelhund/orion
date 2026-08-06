// Builds public/data/site.json from GitHub: repo metadata, the README, the
// releases API and the wiki. Nothing on the page is written by hand — edit the
// orion repo or its wiki, re-run `npm run build`, redeploy.
//
// The generated file is a snapshot: the page renders it instantly on load and
// then refreshes itself against the live GitHub API in the browser (app.js), so
// a new release shows up without a redeploy.

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import {
  escapeHtml,
  extractTitle,
  parseWikiIndex,
  renderMarkdown,
} from '../public/md.js';
import { dropPointerSentences, unlinkUnreachable } from './readme.mjs';
import { featuredRelease, isComplete, latestRelease, PLATFORMS, platformOf } from '../public/versions.js';
import { MIRROR_DIR, SOURCE_REPO_URL } from '../public/config.js';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const TEMPLATE = join(root, 'src', 'index.template.html');
const OUT = join(root, 'public', 'data', 'site.json');
const OUT_HTML = join(root, 'public', 'index.html');
const PLAY_TEMPLATE = join(root, 'src', 'play.template.html');

// Content comes entirely from the local mirror (see scripts/mirror.mjs) —
// the build makes no network calls at all.
const MIRROR = join(root, MIRROR_DIR);
const CHANGELOG = join(MIRROR, 'changelog');

const headers = {
  accept: 'application/vnd.github+json',
  'user-agent': 'orion-home-build',
  ...(process.env.GITHUB_TOKEN
    ? { authorization: `Bearer ${process.env.GITHUB_TOKEN}` }
    : {}),
};

/** README links are repo-relative (`SPEC.md`, `../../releases`), not wiki. */
function resolveReadmeUrl(url) {
  if (/^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(url)) return url;
  const path = url.replace(/^\.?\//, '');
  return path.startsWith('../')
    ? `${SOURCE_REPO_URL}/${path.replace(/^(\.\.\/)+/, '')}`
    : `${SOURCE_REPO_URL}/blob/HEAD/${path}`;
}

// Set from the mirror once the repo's visibility is known.
let sourceIsPublic = true;

const renderReadme = (markdown, options = {}) =>
  renderMarkdown(unlinkUnreachable(markdown, sourceIsPublic), {
    resolveUrl: resolveReadmeUrl,
    ...options,
  });

async function get(url, as = 'json') {
  const res = await fetch(url, { headers });
  if (!res.ok) throw new Error(`${res.status} ${res.statusText} — ${url}`);
  return as === 'json' ? res.json() : res.text();
}

/** Split a markdown document into `## Section` blocks keyed by title. */
function sections(markdown) {
  const out = new Map();
  let title = '';
  let body = [];
  for (const line of markdown.split('\n')) {
    const heading = line.match(/^##\s+(.*)$/);
    if (heading) {
      if (title) out.set(title, body.join('\n').trim());
      title = heading[1].trim();
      body = [];
    } else if (title) {
      body.push(line);
    }
  }
  if (title) out.set(title, body.join('\n').trim());
  return out;
}

/** The first paragraph of a document, after its `# Title`. */
function lede(markdown) {
  const afterTitle = markdown.replace(/^#\s+.*$/m, '');
  const paragraph = afterTitle
    .split('\n\n')
    .map((p) => p.trim())
    .find((p) => p && !p.startsWith('#') && !p.startsWith('!['));
  return paragraph ? paragraph.replace(/\s*\n\s*/g, ' ') : '';
}

/**
 * The wiki's own one-liner is the best marketing copy the project has:
 *   "A StarCraft-style RTS built from scratch in Rust — deterministic lockstep
 *    sim, procedural pixel art and audio, two asymmetric races, ranked ..."
 * Split it into a hero line and the comma-separated claims behind the dash.
 */
function heroCopy(wikiHome, readmeSource) {
  const source = lede(wikiHome);
  const [head, ...tail] = source.split(/\s+[—–]\s+/);

  if (!tail.length) {
    console.warn('  ! wiki lede has no "—" split; falling back to the README');
    const first = lede(readmeSource).split(/(?<=\.)\s+/)[0] || '';
    return { tagline: first, pills: [] };
  }

  const claims = tail
    .join(' — ')
    .split(/(?<=\.)\s/)[0] // drop the trailing "Grab a build from ..." sentence
    .replace(/\.$/, '')
    .split(/,\s+/)
    .map((c) => c.trim())
    .filter(Boolean);

  return {
    tagline: head.trim().replace(/\.$/, ''),
    pills: claims,
  };
}

function normaliseRelease(release) {
  return {
    tag: release.tag_name,
    name: release.name || release.tag_name,
    url: release.html_url,
    publishedAt: release.published_at,
    assets: (release.assets || [])
      .filter((a) => platformOf(a.name)) // skip the web bundle etc.
      .map((a) => ({
        name: a.name,
        size: a.size,
        url: a.browser_download_url,
        platform: platformOf(a.name),
      }))
      .sort(
        (a, b) =>
          PLATFORMS.findIndex((p) => p.key === a.platform) -
          PLATFORMS.findIndex((p) => p.key === b.platform),
      ),
  };
}

/** README "Install" table: | OS | File | Steps | -> per-platform guidance. */
function installGuide(readme) {
  const install = sections(readme).get('Install') || '';
  const rows = install
    .split('\n')
    .filter((l) => /^\s*\|/.test(l) && !/^\s*\|[\s:|-]+\|\s*$/.test(l))
    .map((l) => l.replace(/^\||\|$/g, '').split('|').map((c) => c.trim()));
  const guide = {};
  for (const [os, file, steps] of rows.slice(1)) {
    if (!os || !file) continue;
    guide[platformOf(`${os} ${file}`)] = {
      os,
      file: file.replace(/`/g, ''),
      steps: renderReadme(steps || ''),
    };
  }
  return guide;
}

/** Read a mirrored wiki page, or '' when the mirror does not have it. */
async function changelogPage(version) {
  try {
    return await readFile(join(CHANGELOG, `${version}.md`), 'utf8');
  } catch {
    console.warn(`  ! ${version}.md missing from the mirror`);
    return '';
  }
}

async function buildChangelog(home) {
  return Promise.all(
    parseWikiIndex(home).map(async (entry) => {
      const page = await changelogPage(entry.version);
      return {
        version: entry.version,
        highlights: entry.highlights,
        title: extractTitle(page),
        html: page ? renderMarkdown(page, { skipFirstH1: true, headingOffset: 2 }) : '',
      };
    }),
  );
}

// ---------------------------------------------------------------- rendering

const fmtDate = (iso) =>
  new Date(iso).toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
const fmtSize = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;

// "v0.4.0 / v0.4.1 — The console rework" -> "The console rework"
const stripVersion = (title) =>
  title.replace(/^v[\d.]+(?:\s*\/\s*v[\d.]+)*\s*[—–-]\s*/, '');

/**
 * The hero already says the README's opening sentence — drop it from the
 * description so the two aren't printed back to back.
 */
function introAfterHero(readmeSource, heroTagline) {
  const paragraph = lede(readmeSource);
  const [first, ...rest] = paragraph.split(/(?<=\.)\s+/);
  const same = (text) => text.replace(/[.\s]+$/, '').toLowerCase();
  return rest.length && same(first) === same(heroTagline)
    ? rest.join(' ')
    : paragraph;
}

function renderDownloads(release, install) {
  if (!release) return '<p class="section-lede">No published release yet.</p>';
  return release.assets
    .map((asset) => {
      const guide = install[asset.platform] || {};
      const label = PLATFORMS.find((p) => p.key === asset.platform)?.label || asset.platform;
      return [
        `<div class="dl frame" data-platform="${asset.platform}">`,
        `<h3>${escapeHtml(guide.os || label)}</h3>`,
        `<p class="file">${escapeHtml(asset.name)}</p>`,
        `<div class="steps">${guide.steps || ''}</div>`,
        `<a class="btn btn-primary" href="${asset.url}">`,
        `<span class="btn-label">Download</span>`,
        `<span class="btn-sub">${release.tag} · ${fmtSize(asset.size)}</span></a>`,
        `</div>`,
      ].join('');
    })
    .join('\n');
}

function renderChangelog(changelog) {
  return changelog
    .map((entry) =>
      [
        `<details class="release" id="${entry.version.toLowerCase()}">`,
        `<summary><span class="ver">${escapeHtml(entry.version)}</span>`,
        `<span class="sum">${escapeHtml(stripVersion(entry.title))}</span>`,
        `<span class="hl">${escapeHtml(entry.highlights)}</span></summary>`,
        `<div class="release-body">${entry.html}</div>`,
        `</details>`,
      ].join(''),
    )
    .join('\n');
}

/** Fill the browser-build page. Same slot rules as the main template. */
function renderPlayPage(template, site) {
  const slots = {
    playVersion: site.play.version,
    wasmSize: fmtSize(site.play.wasmBytes),
    playDescription:
      `Play Orion ${site.play.version} in the browser — no install. ` +
      'Needs a WebGPU browser (Chrome or Edge).',
    playNote:
      'Runs entirely on your machine. Same build as the desktop download, so ' +
      'browser and desktop players can share lobbies. Ranked matchmaking and ' +
      'replay upload are desktop-only.',
  };
  return template.replace(/\{\{(\w+)\}\}/g, (_, key) => {
    if (!(key in slots)) throw new Error(`Play template slot {{${key}}} has no value`);
    return String(slots[key]);
  });
}

function renderPage(template, site) {
  const chip = [
    site.release?.tag,
    'free',
    site.repo.license,
  ].filter(Boolean).join(' · ');

  const slots = {
    title: `Orion — ${site.hero.tagline}`,
    metaDescription: site.tagline.slice(0, 300),
    versionChip: chip,
    heroTagline: escapeHtml(site.hero.tagline),
    pills: site.hero.pills.map((p) => `<li>${escapeHtml(p)}</li>`).join(''),
    license: site.repo.license || 'open source',
    intro: site.intro,
    releaseTag: site.release?.tag || '',
    releaseDate: site.release ? ` · ${fmtDate(site.release.publishedAt)}` : '',
    playNav: site.play ? '<a class="play-only" href="/play">Play</a>' : '',
    releasesUrl: site.repo.releasesUrl,
    // Nav + footer always point at the repo; the hero's second button becomes
    // "play in browser" when a matching web build exists.
    repoUrl: site.links.primary,
    heroBtnClass: site.play ? ' play-only' : '',
    heroBtnUrl: site.play ? '/play' : site.links.primary,
    heroBtnLabel: site.play ? 'Play in browser' : site.links.primaryLabel,
    heroBtnSub: site.play ? 'No install · Chrome or Edge' : site.links.primarySub,
    repoFullName: site.links.credit,
    wikiUrl: site.links.wiki,
    downloads: renderDownloads(site.release, site.install),
    changelog: renderChangelog(site.changelog),
    footer:
      `${site.repo.license || 'Open source'} · written in ${site.repo.language} · ` +
      `${site.releaseCount} releases`,
    builtOn: fmtDate(site.generatedAt),
  };

  return template.replace(/\{\{(\w+)\}\}/g, (_, key) => {
    if (!(key in slots)) throw new Error(`Template slot {{${key}}} has no value`);
    return String(slots[key]);
  });
}

async function main() {
  if (!existsSync(MIRROR)) {
    throw new Error(`no ${MIRROR_DIR}/ directory — run \`npm run mirror\` first`);
  }
  console.log(`Building from ${MIRROR_DIR}/…`);

  const [repo, readme, wikiHome, releases] = await Promise.all([
    readFile(join(MIRROR, 'repo.json'), 'utf8').then(JSON.parse),
    readFile(join(MIRROR, 'README.md'), 'utf8'),
    readFile(join(CHANGELOG, 'Home.md'), 'utf8'),
    readFile(join(MIRROR, 'releases.json'), 'utf8').then(JSON.parse),
  ]);

  const published = releases.filter((r) => !r.draft && !r.prerelease);
  const newest = featuredRelease(releases);
  const absoluteNewest = latestRelease(releases);
  if (absoluteNewest && newest && absoluteNewest.tag_name !== newest.tag_name) {
    console.warn(
      `  ! ${absoluteNewest.tag_name} is missing platform builds — featuring ` +
        `${newest.tag_name} so every platform stays on one version`,
    );
  }
  const changelog = await buildChangelog(wikiHome);
  sourceIsPublic = repo.sourceIsPublic;
  const hero = heroCopy(wikiHome, readme);

  // Once the game's repo is private, every link a visitor can click has to
  // point at the public mirror instead — including the "view source" button,
  // which becomes a downloads link because there is no source to show.
  const links = repo.sourceIsPublic
    ? {
        primary: SOURCE_REPO_URL,
        primaryLabel: 'View source',
        primarySub: `GitHub · ${repo.license || 'open source'}`,
        wiki: `${SOURCE_REPO_URL}/wiki`,
        credit: repo.fullName,
      }
    : {
        primary: '#download',
        primaryLabel: 'All downloads',
        primarySub: 'Hosted right here',
        wiki: '#changelog',
        credit: repo.fullName,
      };

  const site = {
    generatedAt: new Date().toISOString(),
    sourceIsPublic: repo.sourceIsPublic,
    links,
    repo: {
      name: repo.name,
      fullName: repo.fullName,
      url: links.primary,
      description: repo.description || '',
      language: repo.language,
      license: repo.license || '',
      wikiUrl: links.wiki,
      releasesUrl: '#download',
    },
    tagline: repo.description || lede(readme),
    hero,
    intro: renderReadme(dropPointerSentences(introAfterHero(readme, hero.tagline), sourceIsPublic)),
    install: installGuide(readme),
    release: newest ? normaliseRelease(newest) : null,
    releaseCount: published.length,
    changelog,
  };

  // The browser build, if the mirror has one that matches this release.
  const web = existsSync(join(MIRROR, 'web.json'))
    ? JSON.parse(await readFile(join(MIRROR, 'web.json'), 'utf8'))
    : null;
  // The wasm is gitignored (a new 3 MB blob every release), so a build from a
  // fresh clone has web.json but no bundle. Never link to a /play that is not
  // actually there.
  const bundlePresent = (web?.files || []).every((f) =>
    existsSync(join(root, 'public', 'play', 'dist', f)),
  );
  site.play = web && web.version === site.release?.tag && bundlePresent ? web : null;

  if (web && !bundlePresent) {
    console.warn('  ! web bundle missing from public/play/dist — run `npm run mirror`; /play hidden');
  } else if (web && !site.play) {
    console.warn(
      `  ! web build is ${web.version} but the release is ${site.release?.tag} — /play hidden`,
    );
  }

  if (site.play) {
    await writeFile(
      join(root, 'public', 'play', 'index.html'),
      renderPlayPage(await readFile(PLAY_TEMPLATE, 'utf8'), site),
    );
    console.log(`  web build  ${site.play.version} (${fmtSize(site.play.wasmBytes)})`);
  }

  await mkdir(dirname(OUT), { recursive: true });
  await writeFile(OUT, `${JSON.stringify(site, null, 2)}\n`);
  await writeFile(OUT_HTML, renderPage(await readFile(TEMPLATE, 'utf8'), site));

  console.log(`  release   ${site.release?.tag} (${site.release?.assets.length} assets)`);
  console.log(`  hero      "${site.hero.tagline}" + ${site.hero.pills.length} pills`);
  console.log(`  changelog ${site.changelog.length} versions`);
  console.log(`Wrote ${OUT}\nWrote ${OUT_HTML}`);
}

await main();
