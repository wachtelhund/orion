// Handling for README links once the game's repo is private.
//
// The README is written for people looking at the repo, so it links around it
// relatively (`SPEC.md`, `docs/wiki/Home.md`). Those resolve into a repo the
// site's visitors cannot see, so they must never be rendered as links here.

/** Relative to the repo, i.e. not an absolute URL or an in-page anchor. */
export const isRepoRelative = (url) => !/^([a-z][a-z0-9+.-]*:|\/\/|#)/i.test(url);

const LINK = /\[([^\]]*)\]\(([^)\s]+)\)/g;

/** Render repo-relative links as plain text rather than as dead links. */
export function unlinkUnreachable(markdown, sourceIsPublic) {
  if (sourceIsPublic) return markdown;
  return markdown.replace(LINK, (whole, label, url) =>
    isRepoRelative(url) ? label : whole,
  );
}

/**
 * Drop sentences that exist only to point into the repo — e.g. "Full version
 * history with screenshots: [docs/wiki](docs/wiki/Home.md)." Unlinking one of
 * those leaves a dangling label ("…screenshots: docs/wiki."), and the site
 * carries the changelog itself, which is where that reader was headed.
 */
export function dropPointerSentences(markdown, sourceIsPublic) {
  if (sourceIsPublic) return markdown;
  return markdown
    .split(/(?<=\.)\s+/)
    .filter((sentence) => {
      const links = [...sentence.matchAll(LINK)];
      return !links.some((m) => isRepoRelative(m[2]));
    })
    .join(' ');
}
