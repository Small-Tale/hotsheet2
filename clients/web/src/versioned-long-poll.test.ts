import { describe, expect, it } from 'vitest';

import { startVersionedLongPoll } from './versioned-long-poll';

describe('the versioned long poll (HS2-J89JC7, HS2-S3FKJ7)', () => {
  it('passes the last version, reports each new one once, backs off and forgets it after a failure', async () => {
    const answers: Array<{ version: string } | Error> = [
      { version: 'a' },
      { version: 'a' },
      new Error('down'),
      { version: 'b' },
    ];
    const asked: Array<string | undefined> = [];
    const seen: string[] = [];
    const delays: number[] = [];
    let done: () => void = () => undefined;
    const drained = new Promise<void>((resolve) => {
      done = resolve;
    });
    const stop = startVersionedLongPoll({
      read: (after) => {
        asked.push(after);
        const next = answers.shift();
        if (!next) {
          done();
          return new Promise<{ version: string }>(() => undefined);
        }
        return next instanceof Error ? Promise.reject(next) : Promise.resolve(next);
      },
      onValue: (value) => seen.push(value.version),
      onError: () => undefined,
      delay: (ms) => {
        delays.push(ms);
        return Promise.resolve();
      },
    });
    await drained;
    stop();
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
