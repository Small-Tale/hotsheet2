import type { AnnouncerReelItem } from './announcer-reel';

/**
 * What the announcer says for an entry, given what it said last (HS2-MR8GA4; `docs/15` §15.6). When an
 * utterance is about a different project than the one before it, or is the first since the announcer
 * opened, it names the project first: "For Hot Sheet 2, I pushed changes to main." Consecutive
 * utterances about the same project are said as they are. Pure: the caller keeps the last project.
 */
export function announcerUtterance(
  entry: Pick<AnnouncerReelItem, 'projectId' | 'projectName' | 'spoken'>,
  lastProjectId: string | undefined,
): string {
  if (entry.projectId === lastProjectId) return entry.spoken;
  return forProject(entry.projectName, entry.spoken);
}

/** "For <project>, <sentence>", lowering the sentence's first letter unless it starts with "I", a
 * proper name in capitals, or an acronym. An unnamed project leaves the sentence as it is. */
export function forProject(projectName: string, spoken: string): string {
  const name = projectName.trim();
  const sentence = spoken.trim();
  if (!name || !sentence) return sentence;
  return `For ${name}, ${lowerLead(sentence)}`;
}

function lowerLead(sentence: string): string {
  const word = /^\S+/.exec(sentence)?.[0] ?? '';
  // "I", "I've", and words with a capital after the first letter (HS2, QXVHPB, GitHub) keep their case.
  if (/^I\b/.test(word) || /^.\S*[A-Z]/.test(word)) return sentence;
  return sentence.charAt(0).toLowerCase() + sentence.slice(1);
}
