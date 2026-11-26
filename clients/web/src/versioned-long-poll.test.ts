import { describe, expect, it } from 'vitest';

import { EARLY_ANSWER_FLOOR_MS, startVersionedLongPoll, versionedLongPollBackoff } from './versioned-long-poll';

type Answer = { version: string; holdMs?: number } | Error;

/** Drives a poll over scripted answers on a fake clock; each answer advances the clock by its hold. */
async function drive(answers: Answer[]) {
  const script = [...answers];
  const asked: Array<string | undefined> = [];
  const seen: string[] = [];
  const delays: number[] = [];
  let clock = 0;
  let done: () => void = () => undefined;
  const drained = new Promise<void>((resolve) => {
    done = resolve;
  });
  const stop = startVersionedLongPoll({
    read: (after) => {
      asked.push(after);
      const next = script.shift();
      if (!next) {
        done();
        return new Promise<{ version: string }>(() => undefined);
      }
      if (next instanceof Error) return Promise.reject(next);
      clock += next.holdMs ?? 0;
      return Promise.resolve({ version: next.version });
    },
    onValue: (value) => seen.push(value.version),
    onError: () => undefined,
    delay: (ms) => {
      delays.push(ms);
      clock += ms;
      return Promise.resolve();
    },
    now: () => clock,
  });
  await drained;
  stop();
  return { asked, seen, delays };
}

const held = 25_000;

describe('the versioned long poll (HS2-J89JC7, HS2-S3FKJ7)', () => {
  it('passes the last version, reports each new one once, backs off and forgets it after a failure', async () => {
    const { asked, seen, delays } = await drive([
      { version: 'a' },
      { version: 'a', holdMs: held },
      new Error('down'),
      { version: 'b' },
    ]);
    expect(asked).toEqual([undefined, 'a', 'a', undefined, 'b']);
    expect(seen).toEqual(['a', 'b']);
    expect(delays).toEqual([1_000]);
  });

  it('aborts the open read and reports nothing after stopping', async () => {
    let abort: AbortSignal | undefined;
    let answer: (value: { version: string }) => void = () => undefined;
    const seen: string[] = [];
    const stop = startVersionedLongPoll({
      read: (_after, signal) => {
        abort = signal;
        return new Promise<{ version: string }>((resolve) => {
          answer = resolve;
        });
      },
      onValue: (value) => seen.push(value.version),
      onError: () => undefined,
    });
    stop();
    expect(abort?.aborted).toBe(true);
    answer({ version: 'late' });
    await Promise.resolve();
    expect(seen).toEqual([]);
  });
});

describe('the early-answer floor (HS2-YA6D31)', () => {
  it('backs off an unchanged answer that did not hold, keeping the version', async () => {
    const { asked, seen, delays } = await drive([
      { version: 'a' },
      { version: 'a', holdMs: 0 },
      { version: 'a', holdMs: EARLY_ANSWER_FLOOR_MS - 1 },
      { version: 'a', holdMs: 5 },
    ]);
    expect(seen).toEqual(['a']);
    expect(asked).toEqual([undefined, 'a', 'a', 'a', 'a']);
    expect(delays).toEqual([1_000, 2_000, 4_000]);
  });

  it('does not wait after an unchanged answer that held at least the floor', async () => {
    const { delays } = await drive([
      { version: 'a' },
      { version: 'a', holdMs: EARLY_ANSWER_FLOOR_MS },
      { version: 'a', holdMs: held },
    ]);
    expect(delays).toEqual([]);
  });

  it('caps repeated failures at 30 s', async () => {
    const { delays } = await drive(Array.from({ length: 8 }, () => new Error('down')));
    expect(delays).toEqual([1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000]);
    expect(versionedLongPollBackoff(100)).toBe(30_000);
  });

  it('resets the backoff after a changed answer or a held unchanged one', async () => {
    const { delays, seen } = await drive([
      new Error('down'),
      new Error('down'),
      { version: 'a' },
      new Error('down'),
      { version: 'b' },
      { version: 'b', holdMs: 0 },
      { version: 'b', holdMs: 0 },
      { version: 'b', holdMs: held },
      { version: 'b', holdMs: 0 },
    ]);
    expect(seen).toEqual(['a', 'b']);
    expect(delays).toEqual([1_000, 2_000, 1_000, 1_000, 2_000, 1_000]);
  });
});
