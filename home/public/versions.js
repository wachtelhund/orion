// Which release is "latest".
//
// GitHub orders releases (and resolves /releases/latest) by *creation time*,
// which for a mirror is the time it was copied, not the time it was cut. Mirror
// v0.8.0 after v0.9.0 — as happens on any catch-up run — and GitHub would call
// v0.8.0 the latest. So order by version number instead, and never trust the
// API's ordering.

/** `v0.10.2` -> [0, 10, 2]. Unparseable tags sort last. */
export function parseTag(tag) {
  const match = String(tag).match(/(\d+)(?:\.(\d+))?(?:\.(\d+))?/);
  if (!match) return null;
  return [match[1], match[2], match[3]].map((n) => Number(n ?? 0));
}

/** Descending comparator: newest version first. */
export function compareTags(a, b) {
  const [pa, pb] = [parseTag(a), parseTag(b)];
  if (!pa && !pb) return 0;
  if (!pa) return 1;
  if (!pb) return -1;
  for (let i = 0; i < 3; i++) {
    if (pa[i] !== pb[i]) return pb[i] - pa[i];
  }
  return 0;
}

/** Newest published release by version number, or null if there are none. */
export function latestRelease(releases) {
  const published = (releases || []).filter((r) => !r.draft && !r.prerelease);
  if (!published.length) return null;
  return [...published].sort((a, b) => compareTags(a.tag_name, b.tag_name))[0];
}

export const PLATFORMS = [
  { key: 'macos', label: 'macOS', test: /mac|darwin|osx/i },
  { key: 'windows', label: 'Windows', test: /win/i },
  { key: 'linux', label: 'Linux', test: /linux/i },
];

/** Desktop platform an asset is for, or null (e.g. the web demo bundle). */
export const platformOf = (name) =>
  PLATFORMS.find((p) => p.test.test(name))?.key ?? null;

/** Does this release carry a build for every desktop platform? */
export function isComplete(release) {
  const got = new Set(
    (release?.assets || []).map((a) => platformOf(a.name)).filter(Boolean),
  );
  return PLATFORMS.every((p) => got.has(p.key));
}

/**
 * The release to actually feature.
 *
 * A release whose CI partly failed can be missing platforms — v0.20.0 shipped
 * macOS only. Featuring it would leave Windows and Linux visitors with nothing
 * to download, and mixing platforms across versions is worse than it sounds:
 * PROTOCOL_VERSION is the crate version, so a Windows player on an older build
 * cannot join a macOS player on a newer one. So: newest release that is
 * complete, keeping everyone on one version. Falls back to the newest release
 * if none is complete.
 */
export function featuredRelease(releases) {
  const published = (releases || [])
    .filter((r) => !r.draft && !r.prerelease)
    .sort((a, b) => compareTags(a.tag_name, b.tag_name));
  return published.find(isComplete) ?? published[0] ?? null;
}
