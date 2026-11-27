import { describe, expect, it } from 'vitest';

import { announcerUtterance, forProject } from './announcer-utterance';

const entry = (projectId: string, projectName: string, spoken: string) => ({ projectId, projectName, spoken });

describe('the announcer names the project it starts talking about (HS2-MR8GA4)', () => {
  it('names the project on the first utterance and on every change of project', () => {
    const said: string[] = [];
    let last: string | undefined;
    for (const next of [
      entry('hs2', 'Hot Sheet 2', 'codex pushed changes to main.'),
      entry('hs2', 'Hot Sheet 2', 'codex ran the tests, and they passed.'),
      entry('kerf', 'Kerf', 'I have a question about color preferences, when you have a moment.'),
      entry('hs2', 'Hot Sheet 2', 'I claimed a ticket.'),
      entry('hs2', 'Hot Sheet 2', 'I committed changes.'),
    ]) {
      said.push(announcerUtterance(next, last));
      last = next.projectId;
    }
    expect(said).toEqual([
      'For Hot Sheet 2, codex pushed changes to main.',
      'codex ran the tests, and they passed.',
      'For Kerf, I have a question about color preferences, when you have a moment.',
      'For Hot Sheet 2, I claimed a ticket.',
      'I committed changes.',
    ]);
  });

  it('starts over after a reset of the last project', () => {
    const next = entry('hs2', 'Hot Sheet 2', 'I committed changes.');
    expect(announcerUtterance(next, 'hs2')).toBe('I committed changes.');
    expect(announcerUtterance(next, undefined)).toBe('For Hot Sheet 2, I committed changes.');
  });

  it('keeps the case of "I", acronyms, and capitalized names, and lowers an ordinary first word', () => {
    expect(forProject('Kerf', "I've created a new ticket.")).toBe("For Kerf, I've created a new ticket.");
    expect(forProject('Kerf', 'HS2-ABCDEF is blocked.')).toBe('For Kerf, HS2-ABCDEF is blocked.');
    expect(forProject('Kerf', 'GitHub sync failed.')).toBe('For Kerf, GitHub sync failed.');
    expect(forProject('Kerf', 'Coalesced 3 additional activity events')).toBe(
      'For Kerf, coalesced 3 additional activity events',
    );
    expect(forProject('Kerf', 'It is done.')).toBe('For Kerf, it is done.');
  });

  it('says the sentence alone when the project has no name or there is nothing to say', () => {
    expect(forProject('  ', 'I committed changes.')).toBe('I committed changes.');
    expect(forProject('Kerf', '   ')).toBe('');
  });
});
