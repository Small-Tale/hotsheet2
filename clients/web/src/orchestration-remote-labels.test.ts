import { describe, expect, it } from 'vitest';

import {
  createRemoteLabelReader,
  missingRemoteLabels,
  remoteLabelKey,
  remoteTicketLabel,
  withRemoteLabels,
} from './orchestration-remote-labels';
import type { OrchestrationTimeline, TimelineItem } from './orchestration-timeline';
import { demoTimeline } from './ux-demo/orchestration-fixtures';

/** The running demo with its items made remote and untitled from `from` on (or all of them). */
function remote(predicate: (item: TimelineItem, index: number) => boolean = () => true): OrchestrationTimeline {
  let index = 0;
  const view = demoTimeline('running');
  return {
    ...view,
    waves: view.waves.map((wave) => ({
      ...wave,
      items: wave.items.map((item) => {
        const chosen = predicate(item, index++);
        return chosen ? { ...item, location: 'remote' as const, title: undefined } : item;
      }),
    })),
  };
}
const items = (view: OrchestrationTimeline) => view.waves.flatMap((wave) => wave.items);

describe('remote target labels on the timeline (HS2-HDHT5E)', () => {
  it('asks only for remote items without a title, each once, in order, up to the limit', () => {
    const view = remote((_item, index) => index % 2 === 0);
    const wanted = items(view).filter((item) => item.location === 'remote' && item.title === undefined);
    const keys = [...new Set(wanted.map((item) => remoteLabelKey(item.target)))];
    expect(missingRemoteLabels(view, new Set()).map(remoteLabelKey)).toEqual(keys);
    expect(missingRemoteLabels(view, new Set(), 1).map(remoteLabelKey)).toEqual(keys.slice(0, 1));
    // Known (read, failed or in flight) keys are not asked again.
    expect(missingRemoteLabels(view, new Set(keys))).toEqual([]);
    // Local items and items with a title never ask.
    expect(missingRemoteLabels(demoTimeline('running'), new Set())).toEqual([]);
    expect(missingRemoteLabels(undefined, new Set())).toEqual([]);
  });

  it('fills a remote item’s title and missing slug, and leaves everything else as it was', () => {
    const view = remote();
    const first = items(view)[0];
    const key = remoteLabelKey(first.target);
    const labelled = withRemoteLabels(view, { [key]: { title: 'Remote title', slug: 'API-9' } })!;
    const filled = items(labelled).find((item) => item.step_id === first.step_id)!;
    expect(filled.title).toBe('Remote title');
    expect(filled.slug).toBe(first.slug ?? 'API-9');
    // Untouched items keep their identity, and no label returns the same timeline.
    expect(items(labelled).filter((item) => item.step_id !== first.step_id)).toEqual(
      items(view).filter((item) => item.step_id !== first.step_id),
    );
    expect(withRemoteLabels(view, {})).toBe(view);
    expect(withRemoteLabels(view, { other: { title: 'x' } })).toBe(view);
    expect(withRemoteLabels(undefined, { [key]: { title: 'x' } })).toBeUndefined();
    // A label without a title fills nothing; a local item is never relabelled.
    expect(withRemoteLabels(view, { [key]: { slug: 'API-9' } })).toBe(view);
    const local = demoTimeline('running');
    expect(withRemoteLabels(local, { [remoteLabelKey(items(local)[0].target)]: { title: 'x' } })).toBe(local);
  });

  it('reads a label from a ticket read, and nothing from an answer without a title', () => {
    expect(remoteTicketLabel({ slug: 'API-9', title: 'Remote', details: 'x' })).toEqual({
      slug: 'API-9',
      title: 'Remote',
    });
    expect(remoteTicketLabel({ title: 'Remote', slug: 7 })).toEqual({ title: 'Remote', slug: undefined });
    for (const answer of [null, 'x', {}, { title: '' }, { title: 3 }])
      expect(remoteTicketLabel(answer)).toBeUndefined();
  });
});

describe('the remote label reader (HS2-HDHT5E)', () => {
  /** A target with `count` remote, untitled items, each its own ticket. */
  function many(count: number): OrchestrationTimeline {
    const view = demoTimeline('running'),
      base = view.waves.flatMap((wave) => wave.items)[0];
    return {
      ...view,
      waves: [
        {
          kind: 'next',
          ordinal: 0,
          items: Array.from({ length: count }, (_, index) => ({
            ...base,
            step_id: `step-r${index}`,
            location: 'remote' as const,
            title: undefined,
            target: { ...base.target, native_id: `remote-${index}` },
          })),
        },
      ],
    };
  }

  it('reads each missing label once, four at a time, at most 24 per timeline version, and skips failures', async () => {
    const pending: Array<{ ref: { native_id: string }; resolve: (value: unknown) => void; reject: () => void }> = [];
    const seen: Array<Readonly<Record<string, { title?: string }>>> = [];
    const reader = createRemoteLabelReader({
      read: (ref) =>
        new Promise((resolve, reject) => {
          pending.push({
            ref,
            resolve,
            reject: () => {
              reject(new Error('down'));
            },
          });
        }),
      onLabels: (labels) => seen.push(labels),
    });
    const settle = async () => {
      for (let turn = 0; turn < 6; turn += 1) await Promise.resolve();
    };
    const view = many(30);
    reader.fill(view);
    expect(pending.map((call) => call.ref.native_id)).toEqual(['remote-0', 'remote-1', 'remote-2', 'remote-3']);
    pending[0].resolve({ title: 'First', slug: 'API-0' });
    pending[1].reject();
    pending[2].resolve({});
    await settle();
    // Three settled, so three more started; only the titled answer became a label.
    expect(pending).toHaveLength(7);
    expect(seen.at(-1)).toEqual({
      [remoteLabelKey(view.waves[0].items[0].target)]: { title: 'First', slug: 'API-0' },
    });
    // This version queued 24; draining them reads nothing more until the next version asks.
    for (let index = 3; index < 24; index += 1) {
      pending[index].resolve({ title: `T${index}` });
      await settle();
    }
    expect(pending).toHaveLength(24);
    // The next version asks for the six left; the failed and the untitled ones stay known.
    reader.fill(view);
    for (let index = 24; index < 30; index += 1) {
      await settle();
      pending[index].resolve({ title: `T${index}` });
    }
    await settle();
    expect(pending.map((call) => call.ref.native_id)).toEqual(Array.from({ length: 30 }, (_, n) => `remote-${n}`));
    reader.fill(view);
    expect(pending).toHaveLength(30);
    expect(Object.keys(seen.at(-1)!)).toHaveLength(28);
  });

  it('forgets everything on reset and ignores answers that arrive after it', async () => {
    const pending: Array<(value: unknown) => void> = [];
    const signals: AbortSignal[] = [];
    const seen: unknown[] = [];
    const reader = createRemoteLabelReader({
      read: (_ref, signal) => {
        signals.push(signal);
        return new Promise((resolve) => pending.push(resolve));
      },
      onLabels: (labels) => seen.push(labels),
    });
    const view = many(2);
    reader.fill(view);
    reader.reset();
    expect(signals.every((signal) => signal.aborted)).toBe(true);
    pending[0]({ title: 'Late' });
    for (let turn = 0; turn < 4; turn += 1) await Promise.resolve();
    expect(seen).toEqual([]);
    // Empty, then refilled: the same tickets are read again.
    reader.fill(view);
    expect(pending).toHaveLength(4);
    pending[2]({ title: 'Again' });
    for (let turn = 0; turn < 4; turn += 1) await Promise.resolve();
    expect(seen).toHaveLength(1);
  });
});
