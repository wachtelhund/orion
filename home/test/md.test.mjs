import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  escapeHtml,
  extractTitle,
  parseWikiIndex,
  renderMarkdown,
  resolveWikiUrl,
  WIKI_RAW_BASE,
} from '../public/md.js';

test('escapes HTML-significant characters', () => {
  assert.equal(escapeHtml('<img src=x onerror="a">'), '&lt;img src=x onerror=&quot;a&quot;&gt;');
});

test('raw HTML in wiki source is neutralised, not passed through', () => {
  assert.match(renderMarkdown('<script>alert(1)</script>'), /&lt;script&gt;/);
  assert.doesNotMatch(renderMarkdown('<script>alert(1)</script>'), /<script>/);
});

test('resolves relative wiki paths, leaves absolute URLs alone', () => {
  assert.equal(resolveWikiUrl('images/v04-console.png'), `${WIKI_RAW_BASE}/images/v04-console.png`);
  assert.equal(resolveWikiUrl('https://example.com/a.png'), 'https://example.com/a.png');
  assert.equal(resolveWikiUrl('#downloads'), '#downloads');
});

test('renders headings with an offset and can drop the page title', () => {
  const md = '# Title\n\n## Section\n';
  assert.equal(renderMarkdown(md), '<h1>Title</h1>\n<h2>Section</h2>');
  assert.equal(
    renderMarkdown(md, { skipFirstH1: true, headingOffset: 1 }),
    '<h3>Section</h3>',
  );
});

test('renders inline emphasis, code and links', () => {
  const html = renderMarkdown('**bold** and *slanted* and `units.ron` and [docs](https://x.dev)');
  assert.match(html, /<strong>bold<\/strong>/);
  assert.match(html, /<em>slanted<\/em>/);
  assert.match(html, /<code>units\.ron<\/code>/);
  assert.match(html, /<a href="https:\/\/x\.dev" target="_blank" rel="noopener noreferrer">docs<\/a>/);
});

test('markup inside a code span is not interpreted', () => {
  const html = renderMarkdown('use `**not bold**` here');
  assert.match(html, /<code>\*\*not bold\*\*<\/code>/);
  assert.doesNotMatch(html, /<strong>/);
});

test('italics survive a paragraph that wraps across lines', () => {
  const html = renderMarkdown('*(v0.4.1 is the fixed\nbuild, patched.)*');
  assert.match(html, /<em>\(v0\.4\.1 is the fixed build, patched\.\)<\/em>/);
});

test('rewrites relative image sources and lazy-loads them', () => {
  const html = renderMarkdown('![The console](images/v04-console.png)');
  assert.match(html, new RegExp(`<img src="${WIKI_RAW_BASE}/images/v04-console\\.png"`));
  assert.match(html, /loading="lazy"/);
});

test('renders nested bullet lists', () => {
  const html = renderMarkdown('- outer\n  - inner\n- second\n');
  assert.equal(html, '<ul><li>outer<ul><li>inner</li></ul></li><li>second</li></ul>');
});

test('an indented non-bullet line continues the current list item', () => {
  const html = renderMarkdown('- Navy tech panels with brushed\n  texture and gold piping.\n');
  assert.equal(html, '<ul><li>Navy tech panels with brushed texture and gold piping.</li></ul>');
});

test('renders tables', () => {
  const html = renderMarkdown('| OS | File |\n|---|---|\n| macOS | `Orion.zip` |\n');
  assert.match(html, /<thead><tr><th>OS<\/th><th>File<\/th><\/tr><\/thead>/);
  assert.match(html, /<td>macOS<\/td><td><code>Orion\.zip<\/code><\/td>/);
});

test('renders fenced code blocks verbatim', () => {
  const html = renderMarkdown('```sh\ncargo run --release\n```\n');
  assert.equal(html, '<pre><code>cargo run --release</code></pre>');
});

test('a horizontal rule is not confused with a table separator', () => {
  assert.equal(renderMarkdown('---'), '<hr>');
});

test('extracts the page title', () => {
  assert.equal(extractTitle('# v0.4.0 — The console rework\n\nBody.'), 'v0.4.0 — The console rework');
  assert.equal(extractTitle('No heading here'), '');
});

test('parses the wiki version index in order', () => {
  const home = [
    '| Version | Highlights |',
    '|---|---|',
    '| [[v0.4.0]] | Console rework |',
    '| [[v0.3.0]] | Ranked matchmaking |',
  ].join('\n');
  assert.deepEqual(parseWikiIndex(home), [
    { version: 'v0.4.0', highlights: 'Console rework' },
    { version: 'v0.3.0', highlights: 'Ranked matchmaking' },
  ]);
});

test('wiki links become in-page anchors', () => {
  assert.match(renderMarkdown('See [[v0.3.0]] for details'), /<a href="#v0\.3\.0">v0\.3\.0<\/a>/);
});

// --- release ordering -------------------------------------------------------

import { compareTags, latestRelease, parseTag } from '../public/versions.js';

const rel = (tag, extra = {}) => ({ tag_name: tag, ...extra });

test('parses version tags', () => {
  assert.deepEqual(parseTag('v0.10.2'), [0, 10, 2]);
  assert.deepEqual(parseTag('v1.2'), [1, 2, 0]);
  assert.equal(parseTag('nightly'), null);
});

test('orders versions numerically, not lexically', () => {
  const tags = ['v0.9.0', 'v0.10.0', 'v0.8.0'].sort(compareTags);
  assert.deepEqual(tags, ['v0.10.0', 'v0.9.0', 'v0.8.0']);
});

test('latest ignores the order releases were mirrored in', () => {
  // v0.8.0 copied *after* v0.9.0 — GitHub would call it newest; we must not.
  const list = [rel('v0.8.0'), rel('v0.9.0'), rel('v0.7.0')];
  assert.equal(latestRelease(list).tag_name, 'v0.9.0');
});

test('latest skips drafts and prereleases, and copes with none', () => {
  const list = [rel('v1.0.0', { prerelease: true }), rel('v0.9.0', { draft: true }), rel('v0.8.0')];
  assert.equal(latestRelease(list).tag_name, 'v0.8.0');
  assert.equal(latestRelease([]), null);
});

// --- incomplete releases ----------------------------------------------------

import { featuredRelease, isComplete, platformOf } from '../public/versions.js';

const withAssets = (tag, names) => ({ tag_name: tag, assets: names.map((name) => ({ name })) });
const ALL = ['Orion-macOS.zip', 'orion-windows-x86_64.zip', 'orion-linux-x86_64.tar.gz'];

test('classifies release assets by platform, ignoring non-desktop ones', () => {
  assert.equal(platformOf('Orion-macOS.zip'), 'macos');
  assert.equal(platformOf('orion-windows-x86_64.zip'), 'windows');
  assert.equal(platformOf('orion-linux-x86_64.tar.gz'), 'linux');
  assert.equal(platformOf('orion-web-demo.zip'), null);
});

test('a release is complete only with all three desktop builds', () => {
  assert.equal(isComplete(withAssets('v1.0.0', ALL)), true);
  // v0.20.0's real shape: CI failed, so only macOS and the web bundle shipped.
  assert.equal(isComplete(withAssets('v0.20.0', ['Orion-macOS.zip', 'orion-web-demo.zip'])), false);
});

test('features the newest COMPLETE release, not the newest one', () => {
  const list = [
    withAssets('v0.20.0', ['Orion-macOS.zip', 'orion-web-demo.zip']),
    withAssets('v0.19.0', ALL),
    withAssets('v0.18.0', ALL),
  ];
  assert.equal(featuredRelease(list).tag_name, 'v0.19.0');
});

test('falls back to the newest release when none is complete', () => {
  const list = [withAssets('v0.20.0', ['Orion-macOS.zip']), withAssets('v0.19.0', [])];
  assert.equal(featuredRelease(list).tag_name, 'v0.20.0');
  assert.equal(featuredRelease([]), null);
});

// --- README links once the game repo is private ------------------------------

import { dropPointerSentences, unlinkUnreachable } from '../scripts/readme.mjs';

const LEDE =
  'No game engine — a wgpu renderer. Three asymmetric races, skirmish AI. ' +
  'Full version history with screenshots: [docs/wiki](docs/wiki/Home.md). ' +
  'All art is generated at startup.';

test('while the repo is public, README links are left exactly as written', () => {
  assert.equal(unlinkUnreachable(LEDE, true), LEDE);
  assert.equal(dropPointerSentences(LEDE, true), LEDE);
});

test('repo-relative links become plain text once the repo is private', () => {
  const out = unlinkUnreachable('See [SPEC.md](SPEC.md) for the sim.', false);
  assert.equal(out, 'See SPEC.md for the sim.');
});

test('absolute links and anchors survive going private', () => {
  const md = '[Releases](https://github.com/x/y/releases) and [top](#top)';
  assert.equal(unlinkUnreachable(md, false), md);
});

test('a sentence that only points into the repo is dropped whole', () => {
  const out = dropPointerSentences(LEDE, false);
  assert.doesNotMatch(out, /version history/);
  assert.doesNotMatch(out, /docs\/wiki/);
  // ...without taking the surrounding sentences with it.
  assert.match(out, /No game engine/);
  assert.match(out, /All art is generated at startup\./);
});
