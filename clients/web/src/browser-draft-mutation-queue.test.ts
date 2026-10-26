import { expect, it } from 'vitest';

import { createBrowserDraftMutationQueue } from './browser-draft-mutation-queue';

it('preserves held save/remove/save order while another key progresses (HS2-2HF3H4)', async () => {
  let release: () => void = () => undefined,
    begin: () => void = () => undefined;
  const enqueue = createBrowserDraftMutationQueue(),
    gate = new Promise<void>((resolve) => {
      release = resolve;
    }),
    started = new Promise<void>((resolve) => {
      begin = resolve;
    }),
    events: string[] = [];
  const first = enqueue('a', async () => {
    begin();
    await gate;
    events.push('old save');
  });
  await started;
  const remove = enqueue('a', () => {
      events.push('remove');
      return Promise.resolve();
    }),
    next = enqueue('a', () => {
      events.push('new save');
      return Promise.resolve();
    });
  await enqueue('b', () => {
    events.push('independent');
    return Promise.resolve();
  });
  expect(events).toEqual(['independent']);
  release();
  await Promise.all([first, remove, next]);
  expect(events).toEqual(['independent', 'old save', 'remove', 'new save']);
});

it('propagates a failed preparation but permits queued removal and a later edit (HS2-2HF3H4)', async () => {
  const enqueue = createBrowserDraftMutationQueue(),
    failure = new Error('Synthetic byte preparation failure'),
    events: string[] = [];
  const failed = enqueue('a', () => Promise.reject(failure)),
    removed = enqueue('a', () => {
      events.push('removed');
      return Promise.resolve();
    });
  await expect(failed).rejects.toBe(failure);
  await removed;
  await enqueue('a', () => {
    events.push('edited');
    return Promise.resolve();
  });
  expect(events).toEqual(['removed', 'edited']);
});
