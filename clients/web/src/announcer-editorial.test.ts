import { describe, expect, it } from 'vitest';

import {
  ANNOUNCER_GROUP_WINDOW_MS,
  announcerLevel,
  editAnnouncerReel,
  groupPhrase,
  nextSettleAt,
  spokenList,
} from './announcer-editorial';
import { announcerItem, mergeAnnouncerReel } from './announcer-reel';
import { announcerStep } from './announcer-speech';
import type { ActivityEvent } from './api';

const BASE = Date.parse('2026-11-01T10:00:00Z');
const at = (seconds: number) => new Date(BASE + seconds * 1000).toISOString();
let sequence = 0;

/** A synthetic event `seconds` after the base time. */
function step(seconds: number, kind: string, summary: string, extra: Partial<ActivityEvent> = {}) {
  sequence += 1;
  const id = `e${String(sequence).padStart(3, '0')}`;
  return announcerItem(extra.project ?? 'demo', 'Demo', 'store', {
    id,
    ts: at(seconds),
    tool: 'codex',
    kind,
    summary,
    importance: 'normal',
    ...extra,
  });
}
const command = (seconds: number, text: string, extra: Partial<ActivityEvent> = {}) =>
  step(seconds, 'command', `codex ran \`${text}\``, { detail: { command: text }, ...extra });
const edit = (seconds: number, path: string) => step(seconds, 'edit', `codex edited ${path}`, { detail: { path } });
const settledLater = { now: BASE + 3_600_000 };

describe('announcer editorial levels (HS2-RKYCTP)', () => {
  const level = (item: ReturnType<typeof step>) => announcerLevel(item);
  it.each([
    ['/bin/zsh -lc cargo test', 'headline'],
    ['npm run lint', 'headline'],
    ['npm run build', 'headline'],
    ['npm ci', 'headline'],
    ['git commit -m x', 'headline'],
    ['git push origin main', 'headline'],
    ['git rebase origin/main', 'headline'],
    ['gh pr create --fill', 'headline'],
    ['hotsheet-cli claim HS2-AAAAAA', 'headline'],
    ['hotsheet-cli edit HS2-AAAAAA --status completed', 'headline'],
    ['hotsheet-cli show HS2-AAAAAA', 'routine'],
    ['sed -n 1,20p a.ts', 'routine'],
    ['rg -n overlay', 'routine'],
    ['ls -la', 'routine'],
    ['cd x', 'routine'],
    ['git status', 'routine'],
    ['git diff', 'routine'],
    ['./scripts/sync-docs.sh', 'routine'],
  ])('%s is %s', (text, expected) => {
    expect(level(command(0, text))).toBe(expected);
  });

  it('raises failures, milestones, and high importance and keeps routine kinds routine', () => {
    expect(level(command(0, 'ls missing', { detail: { command: 'ls missing', exit_code: 1 } }))).toBe('headline');
    expect(level(command(0, 'ls', { detail: { command: 'ls', exit_code: 0 } }))).toBe('routine');
    expect(level(command(0, 'ls', { importance: 'high' }))).toBe('headline');
    for (const kind of ['turn_end', 'blocked', 'permission', 'decision', 'plan', 'ticket_status'])
      expect(level(step(0, kind, 'synthetic'))).toBe('headline');
    expect(level(step(0, 'note', 'Background task failed'))).toBe('headline');
    expect(level(step(0, 'note', 'checked the `error` handler'))).toBe('routine');
    expect(level(step(0, 'note', 'synthetic note', { importance: 'low' }))).toBe('routine');
    expect(level(edit(0, 'src/a.ts'))).toBe('routine');
    expect(level(step(0, 'tool_call', 'codex called Task', { detail: { name: 'Task' } }))).toBe('headline');
    expect(level(step(0, 'tool_call', 'codex called Read', { detail: { name: 'Read' } }))).toBe('routine');
    expect(level(step(0, 'turn_start', 'codex started a turn'))).toBe('routine');
  });
});

describe('announcer editorial reel (HS2-RKYCTP)', () => {
  it('folds a rapid burst of routine steps into a few entries and keeps every headline', () => {
    const reel = [
      command(0, '/bin/zsh -lc cd /work && rg -n overlay src'),
      command(1, '/bin/zsh -lc sed -n 1,80p src/reel.ts'),
      command(2, '/bin/zsh -lc ls src'),
      command(3, '/bin/zsh -lc git status --short'),
      edit(4, 'src/reel.ts'),
      command(5, '/bin/zsh -lc sed -n 1,40p src/speech.ts'),
      edit(6, 'src/speech.ts'),
      command(7, '/bin/zsh -lc cd clients/web && npx vitest run', {
        detail: { command: '/bin/zsh -lc cd clients/web && npx vitest run', exit_code: 1 },
      }),
      command(8, '/bin/zsh -lc rg -n failing src'),
      edit(9, 'src/speech.ts'),
      command(10, '/bin/zsh -lc cd clients/web && npx vitest run', {
        detail: { command: '/bin/zsh -lc cd clients/web && npx vitest run', exit_code: 0 },
      }),
      command(11, '/bin/zsh -lc git add src && git commit -m x'),
      command(12, '/bin/zsh -lc git push origin main'),
      step(13, 'turn_end', 'codex finished a turn', { importance: 'high' }),
    ];
    const entries = editAnnouncerReel(reel, settledLater);
    expect(entries.map((entry) => entry.spoken)).toEqual([
      'codex edited reel.ts and speech.ts and looked through the code for overlay.',
      'codex ran the unit tests, and they failed.',
      'codex edited speech.ts and searched the code for failing.',
      'codex ran the unit tests, and they passed.',
      'codex staged changes and committed changes.',
      'codex pushed changes to main.',
      'codex finished a turn',
    ]);
    expect(entries.length).toBeLessThanOrEqual(reel.length / 2);
    // Every reel item is told by exactly one entry, in order.
    expect(entries.flatMap((entry) => entry.members.map((member) => member.key))).toEqual(reel.map((item) => item.key));
    expect(entries.every((entry) => entry.settled)).toBe(true);
  });

  it('never folds or drops a headline, however dense the routine around it', () => {
    const reel = Array.from({ length: 60 }, (_, index) =>
      index % 10 === 9 ? command(index, 'git commit -m x') : command(index, `sed -n 1p f${index}.ts`),
    );
    const entries = editAnnouncerReel(reel, settledLater);
    const headlines = entries.filter((entry) => entry.level === 'headline');
    expect(headlines).toHaveLength(6);
    expect(headlines.every((entry) => entry.members.length === 1 && entry.spoken === 'codex committed changes.')).toBe(
      true,
    );
    expect(entries.filter((entry) => entry.level === 'routine').map((entry) => entry.members.length)).toEqual([
      9, 9, 9, 9, 9, 9,
    ]);
  });

  it('starts a new group after the window, so a long stretch is summarized periodically', () => {
    const reel = Array.from({ length: 30 }, (_, index) => command(index * 5, `rg -n term${index} src`));
    const entries = editAnnouncerReel(reel, settledLater);
    // 150 seconds of searching in 45-second windows: four summaries.
    expect(entries.map((entry) => entry.members.length)).toEqual([9, 9, 9, 3]);
    expect(entries[0].spoken).toBe('codex looked through the code.');
  });

  it('keeps agents apart and the order stable as entries grow', () => {
    const a = command(0, 'rg -n alpha');
    const b = command(1, 'rg -n beta', { tool: 'claude' });
    const a2 = command(2, 'sed -n 1p a.ts');
    const b2 = command(3, 'git commit -m x', { tool: 'claude' });
    const first = editAnnouncerReel([a, b], { now: BASE + 2_000 });
    expect(first.map((entry) => [entry.key, entry.settled])).toEqual([
      [a.key, false],
      [b.key, false],
    ]);
    const second = editAnnouncerReel([a, b, a2, b2], { now: BASE + 4_000 });
    // The codex group keeps its key as it grows; claude's commit closes claude's group only.
    expect(second.map((entry) => [entry.key, entry.members.length, entry.settled])).toEqual([
      [a.key, 2, false],
      [b.key, 1, true],
      [b2.key, 1, true],
    ]);
    expect(second[0].spoken).toBe('codex looked through the code for alpha.');
    // Same input, same output.
    expect(editAnnouncerReel([a, b, a2, b2], { now: BASE + 4_000 })).toEqual(second);
    // Another project or session is another agent.
    const other = command(1, 'rg -n gamma', { project: 'other' });
    const session = command(1, 'rg -n delta', { session: 's2' });
    expect(editAnnouncerReel([a, other, session, a2], settledLater).map((entry) => entry.members.length)).toEqual([
      2, 1, 1,
    ]);
  });

  it('settles an open group when its window passes and says when that is', () => {
    const a = command(0, 'rg -n alpha');
    const b = command(10, 'rg -n beta');
    const open = editAnnouncerReel([a, b], { now: BASE + 20_000 });
    expect(open[0].settled).toBe(false);
    expect(nextSettleAt(open, { now: BASE + 20_000 })).toBe(BASE + ANNOUNCER_GROUP_WINDOW_MS);
    const closed = editAnnouncerReel([a, b], { now: BASE + ANNOUNCER_GROUP_WINDOW_MS });
    expect(closed[0].settled).toBe(true);
    expect(nextSettleAt(closed, { now: BASE + ANNOUNCER_GROUP_WINDOW_MS })).toBeUndefined();
    // Live items run on this device's clock, not the recorder's.
    const receivedAt = new Map([
      [a.key, 5_000_000],
      [b.key, 5_010_000],
    ]);
    const live = editAnnouncerReel([a, b], { now: 5_020_000, receivedAt: (key) => receivedAt.get(key) });
    expect(live[0].members).toHaveLength(2);
    expect(live[0].settled).toBe(false);
    expect(nextSettleAt(live, { now: 5_020_000, receivedAt: (key) => receivedAt.get(key) })).toBe(
      5_000_000 + ANNOUNCER_GROUP_WINDOW_MS,
    );
    // A step that arrives after the window starts the next summary instead of reopening a said one.
    const c = command(20, 'rg -n gamma');
    receivedAt.set(c.key, 5_000_000 + ANNOUNCER_GROUP_WINDOW_MS + 1);
    expect(
      editAnnouncerReel([a, b, c], { now: 5_100_000, receivedAt: (key) => receivedAt.get(key) }).map(
        (entry) => entry.members.length,
      ),
    ).toEqual([2, 1]);
  });

  it('treats an unreadable time as long past', () => {
    const odd = { ...command(0, 'rg -n alpha') };
    odd.event = { ...odd.event, ts: 'not a time' };
    const entries = editAnnouncerReel([odd], { now: BASE });
    expect(entries[0].settled).toBe(true);
    expect(nextSettleAt([{ ...entries[0], settled: false }], { now: BASE })).toBe(ANNOUNCER_GROUP_WINDOW_MS);
  });

  it('shows a group as its summary over a bounded list of its steps', () => {
    const reel = Array.from({ length: 14 }, (_, index) => command(index, `sed -n 1p f${index}.ts`));
    const [entry] = editAnnouncerReel(reel, settledLater);
    expect(entry.narration.split('\n')[0]).toBe('codex looked through the code.');
    expect(entry.narration).toContain('- codex ran `sed -n 1p f0.ts`');
    expect(entry.narration).toContain('- and 2 more');
    expect(entry.narration).not.toContain('f13.ts');
    expect(entry.change).toBeUndefined();
    expect(entry.event).toBe(reel[13].event);
    // A lone step keeps its own display and speech.
    const lone = editAnnouncerReel([edit(0, 'src/sample.ts')], settledLater)[0];
    expect(lone.narration).toBe('codex edited src/sample.ts');
    expect(lone.spoken).toBe('codex edited sample.ts.');
    expect(lone.change?.path).toBe('src/sample.ts');
  });

  it('resets to nothing and refills the same way', () => {
    const reel = [command(0, 'rg -n alpha'), command(1, 'git commit -m x')];
    expect(editAnnouncerReel([], settledLater)).toEqual([]);
    const once = editAnnouncerReel(mergeAnnouncerReel([], reel), settledLater);
    const again = editAnnouncerReel(mergeAnnouncerReel([], [...reel].reverse()), settledLater);
    expect(again).toEqual(once);
  });
});

describe('announcer group phrases (HS2-RKYCTP)', () => {
  const steps = (...items: ReturnType<typeof step>[]) => items.map((item) => announcerStep(item.event));
  it('says what a run of steps did, by intent', () => {
    expect(groupPhrase(steps(edit(0, 'a.ts'), edit(1, 'b.ts'), edit(2, 'c.ts')))).toBe('edited 3 files');
    expect(groupPhrase(steps(edit(0, 'a.ts'), edit(1, 'a.ts')))).toBe('edited a.ts');
    expect(groupPhrase(steps(command(0, 'mkdir out')))).toBe('created a folder');
    expect(groupPhrase(steps(command(0, 'mkdir out'), command(1, 'rm -rf build')))).toBe('changed files');
    expect(groupPhrase(steps(command(0, 'hotsheet-cli show X'), command(1, 'hotsheet-cli ls')))).toBe(
      'worked on tickets',
    );
    expect(groupPhrase(steps(command(0, 'hotsheet-cli show X'), command(1, 'hotsheet-cli show Y')))).toBe(
      'read a ticket',
    );
    expect(
      groupPhrase(steps(command(0, 'make'), command(1, './a-b.sh'), command(2, 'curl x'), command(3, 'open x'))),
    ).toBe('ran make, ran the a b script and took 2 other steps');
    const prose = step(0, 'tool_call', 'Fetched the notes', { detail: { name: 'WebFetch' } });
    expect(groupPhrase(steps(prose, prose))).toBe('took 2 steps');
    expect(groupPhrase(steps(prose, edit(1, 'a.ts')))).toBe('edited a.ts and took one other step');
    expect(groupPhrase(steps(step(0, 'note', 'n'), step(1, 'note', 'm')))).toBe('left a note');
    expect(groupPhrase([])).toBe('kept working');
    expect(spokenList(['a', 'b', 'c'])).toBe('a, b and c');
    expect(spokenList([])).toBe('');
  });
});
