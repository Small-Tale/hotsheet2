import { describe, expect, it } from 'vitest';

import { ApiHttpError } from './api';
import {
  boundedBackoffMs,
  describeRequestFailure,
  isTransientRequestFailure,
  retryTransient,
  SERVER_UNREACHABLE,
} from './bounded-backoff';

describe('the bounded backoff (HS2-JGFPG4)', () => {
  it('waits 1 s doubling to a 30 s cap', () => {
    expect([1, 2, 3, 4, 5, 6, 7, 20].map(boundedBackoffMs)).toEqual([
      1_000, 2_000, 4_000, 8_000, 16_000, 30_000, 30_000, 30_000,
    ]);
    expect(boundedBackoffMs(0)).toBe(1_000);
  });

  it('retries a missing answer, a 5xx, 408 and 429, and no other refusal', () => {
    expect(isTransientRequestFailure(new TypeError('network'))).toBe(true);
    for (const status of [500, 502, 503, 408, 429])
      expect(isTransientRequestFailure(new ApiHttpError('x', status))).toBe(true);
    for (const status of [400, 401, 403, 404, 409, 422])
      expect(isTransientRequestFailure(new ApiHttpError('x', status))).toBe(false);
  });
});

describe('retryTransient (HS2-JGFPG4)', () => {
  function script(answers: Array<string | Error>) {
    let calls = 0;
    const attempt = () => {
      calls += 1;
      const next = answers.shift() ?? 'done';
      return next instanceof Error ? Promise.reject(next) : Promise.resolve(next);
    };
    return { attempt, calls: () => calls };
  }

  it('returns the answer after transient failures, waiting the backoff between them', async () => {
    const delays: number[] = [];
    const s = script([new TypeError('a'), new ApiHttpError('b', 503), 'ok']);
    const answer = await retryTransient(s.attempt, {
      signal: new AbortController().signal,
      delay: (ms) => {
        delays.push(ms);
        return Promise.resolve();
      },
    });
    expect(answer).toBe('ok');
    expect(s.calls()).toBe(3);
    expect(delays).toEqual([1_000, 2_000]);
  });

  it('throws the last failure after the limit, and a refusal at once', async () => {
    const delays: number[] = [];
    const delay = (ms: number) => {
      delays.push(ms);
      return Promise.resolve();
    };
    const last = new TypeError('last');
    const repeated = script([new TypeError('1'), new TypeError('2'), last]);
    await expect(
      retryTransient(repeated.attempt, { signal: new AbortController().signal, delay, maxFailures: 3 }),
    ).rejects.toBe(last);
    expect(repeated.calls()).toBe(3);
    expect(delays).toEqual([1_000, 2_000]);

    const refusal = new ApiHttpError('no', 404);
    const refused = script([refusal]);
    await expect(retryTransient(refused.attempt, { signal: new AbortController().signal, delay })).rejects.toBe(
      refusal,
    );
    expect(refused.calls()).toBe(1);
    expect(delays).toEqual([1_000, 2_000]);
  });

  it('stops without asking again when aborted during the wait or no longer wanted', async () => {
    const controller = new AbortController();
    const aborted = script([new TypeError('a')]);
    await expect(
      retryTransient(aborted.attempt, {
        signal: controller.signal,
        delay: () => {
          controller.abort();
          return Promise.resolve();
        },
      }),
    ).rejects.toBeInstanceOf(TypeError);
    expect(aborted.calls()).toBe(1);

    let wanted = true;
    const unwanted = script([new TypeError('a'), 'late']);
    await expect(
      retryTransient(unwanted.attempt, {
        signal: new AbortController().signal,
        wanted: () => wanted,
        delay: () => {
          wanted = false;
          return Promise.resolve();
        },
      }),
    ).rejects.toBeInstanceOf(TypeError);
    expect(unwanted.calls()).toBe(1);
  });

  it('uses the caller’s own transient test', async () => {
    const s = script([new TypeError('gone')]);
    await expect(
      retryTransient(s.attempt, { signal: new AbortController().signal, transient: () => false }),
    ).rejects.toBeInstanceOf(TypeError);
    expect(s.calls()).toBe(1);
  });
});

describe('describeRequestFailure (HS2-1HGTD7)', () => {
  it('reads a request with no answer the same in every browser', () => {
    expect(SERVER_UNREACHABLE).toBe('The Hot Sheet server could not be reached.');
    for (const text of ['Failed to fetch', 'Load failed', 'NetworkError when attempting to fetch resource.'])
      expect(describeRequestFailure(new TypeError(text))).toBe(SERVER_UNREACHABLE);
  });

  it('keeps the server’s own refusal and any other message', () => {
    expect(describeRequestFailure(new ApiHttpError('The checkout is not open.', 404))).toBe(
      'The checkout is not open.',
    );
    expect(describeRequestFailure(new Error('Unexpected answer.'))).toBe('Unexpected answer.');
    expect(describeRequestFailure('plain')).toBe('plain');
  });
});
