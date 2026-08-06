// Progressive enhancement only. The page is fully rendered at build time from
// GitHub data (scripts/build.mjs); this script keeps it current between
// deploys by re-reading the live GitHub API and wiki, and adds the lightbox.

import {
  extractTitle,
  parseWikiIndex,
  renderMarkdown,
  WIKI_RAW_BASE,
} from './md.js';
import { PLATFORMS, platformOf } from './versions.js';

// Everything the browser reads is same-origin static content deployed with
// this page — never a GitHub repo. No credentials, no rate limits.

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

const formatSize = (bytes) => `${(bytes / 1024 / 1024).toFixed(1)} MB`;
const formatDate = (iso) =>
  new Date(iso).toLocaleDateString(undefined, {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });

function detectPlatform() {
  const hint = `${navigator.userAgentData?.platform || ''} ${navigator.platform || ''} ${navigator.userAgent}`;
  return PLATFORMS.find((p) => p.test.test(hint))?.key ?? null;
}

/** Point the hero button straight at the build for the visitor's OS. */
function wireHeroDownload(release) {
  const button = $('#hero-download');
  if (!button || !release) return;

  const platform = detectPlatform();
  const asset = release.assets.find((a) => a.platform === platform);
  if (!asset) return;

  const label = PLATFORMS.find((p) => p.key === platform).label;
  button.href = asset.url;
  button.removeAttribute('target');
  $('.btn-label', button).textContent = `Download for ${label}`;
  $('#hero-download-sub').textContent = `${release.tag} · ${formatSize(asset.size)} · free`;
}

function renderRelease({ version, highlights, title, html }) {
  const details = document.createElement('details');
  details.className = 'release frame';
  details.id = version.toLowerCase();
  details.innerHTML =
    `<summary><span class="ver">${version}</span>` +
    `<span class="sum"></span><span class="hl"></span></summary>` +
    `<div class="release-body"></div>`;
  // Wiki-authored strings go in as text; only our own rendered HTML as HTML.
  // "v0.4.0 / v0.4.1 — The console rework" -> "The console rework"
  $('.sum', details).textContent = title.replace(
    /^v[\d.]+(?:\s*\/\s*v[\d.]+)*\s*[—–-]\s*/,
    '',
  );
  $('.hl', details).textContent = highlights;
  $('.release-body', details).innerHTML = html;
  return details;
}

/** Add wiki pages published since the last deploy, newest first. */
async function refreshChangelog(known) {
  const list = $('#changelog-list');
  if (!list) return;

  const home = await fetch(`${WIKI_RAW_BASE}/Home.md`).then((r) => {
    if (!r.ok) throw new Error(`wiki Home.md: ${r.status}`);
    return r.text();
  });

  const fresh = parseWikiIndex(home).filter((e) => !known.includes(e.version));
  const pages = await Promise.all(
    fresh.map(async (entry) => {
      const res = await fetch(`${WIKI_RAW_BASE}/${entry.version}.md`);
      if (!res.ok) return null;
      const markdown = await res.text();
      return {
        ...entry,
        title: extractTitle(markdown),
        html: renderMarkdown(markdown, { skipFirstH1: true, headingOffset: 2 }),
      };
    }),
  );

  for (const page of pages.filter(Boolean).reverse()) {
    list.prepend(renderRelease(page));
  }
  if (pages.some(Boolean)) $$('.release', list)[0].open = true;
}

function wireLightbox() {
  const box = $('#lightbox');
  const image = $('img', box);

  document.addEventListener('click', (event) => {
    const target = event.target;
    if (target.matches('.release-body img')) {
      image.src = target.currentSrc || target.src;
      image.alt = target.alt;
      box.hidden = false;
    } else if (!box.hidden && target.closest('#lightbox')) {
      box.hidden = true;
      image.removeAttribute('src');
    }
  });

  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && !box.hidden) box.hidden = true;
  });
}

/** The top bar is transparent over the hero, solid once you scroll past it. */
function wireTopbar() {
  const onScroll = () => {
    document.body.classList.toggle('scrolled', window.scrollY > 80);
  };
  addEventListener('scroll', onScroll, { passive: true });
  onScroll();
}

async function main() {
  wireLightbox();
  wireTopbar();

  const snapshot = await fetch('data/site.json')
    .then((r) => r.json())
    .catch(() => null);
  if (!snapshot) return;

  wireHeroDownload(snapshot.release);

  // Release data is baked into the deploy (mirror -> build -> assets), so
  // there is nothing fresher to fetch. The changelog refresh stays: it reads
  // the same-origin mirror, so a newer deploy's pages appear on soft reloads
  // served from cache. A failure just leaves build-time content in place.
  await Promise.allSettled([
    refreshChangelog(snapshot.changelog.map((c) => c.version)),
  ]);
}

main();
