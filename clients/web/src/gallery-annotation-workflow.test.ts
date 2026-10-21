import { describe, expect, it } from 'vitest';

import type { MediaAnnotation } from './api';
import {
  beginGallerySession,
  completeGallerySave,
  discardGallerySession,
  failGallerySave,
  finishGallerySession,
  type GalleryAnnotationSession,
  type GalleryAnnotationState,
  queueGalleryReset,
  updateGallerySessionOriginal,
} from './gallery-annotation-workflow';

const mark = (text: string): MediaAnnotation => ({ id: 'a', x: 1000, y: 2000, width: 1000, height: 1000, text });
const session = (overrides: Partial<GalleryAnnotationSession> = {}): GalleryAnnotationSession => ({
  projectId: 'a',
  ticketId: 'ticket',
  qualifiedId: 'git:ticket',
  attachmentId: 'image',
  before: [mark('before')],
  original: [mark('before')],
  originalSize: { width: 100, height: 100 },
  ...overrides,
});
const idle: GalleryAnnotationState = { saveState: 'idle' };

describe('gallery annotation workflow (HS2-8SD6XQ)', () => {
  it('consumes an unchanged session and its queued reset without saving', () => {
    let state = beginGallerySession(idle, session());
    state = queueGalleryReset(state, { url: 'next' });
    const finish = finishGallerySession(state, [mark('before')], undefined, false, 'current');
    expect(finish.kind).toBe('unchanged');
    if (finish.kind !== 'unchanged') return;
    expect(finish.reset).toEqual({ url: 'next' });
    expect(finish.state).toEqual(idle);
    expect(finishGallerySession(finish.state, [mark('later')], undefined, false).kind).toBe('none');
  });

  it('keeps a queued close and draft through failure, retry, and successful save', () => {
    let state = beginGallerySession(idle, session());
    state = queueGalleryReset(state, { url: undefined });
    const first = finishGallerySession(state, [mark('edited')], undefined, false);
    expect(first.kind).toBe('save');
    if (first.kind !== 'save') return;
    expect(first.plan).toMatchObject({ method: 'annotations', annotations: [mark('edited')], cropChanged: false });
    state = queueGalleryReset(first.state, { url: 'later' });
    expect(finishGallerySession(state, [mark('newer')], undefined, false).kind).toBe('none');
    state = failGallerySave(state, 'network offline');
    expect(state).toMatchObject({
      saveState: 'failed',
      failure: 'network offline',
      pendingReset: { url: 'later' },
      session: session(),
    });
    const retry = finishGallerySession(state, [mark('edited')], undefined, false);
    expect(retry.kind).toBe('save');
    if (retry.kind !== 'save') return;
    const completed = completeGallerySave(retry.state);
    expect(completed).toEqual({ state: idle, reset: { url: 'later' } });
    expect(discardGallerySession(completed.state).state).toBe(completed.state);
  });

  it('uses markup for a crop or revision and projects visible annotations to canonical coordinates', () => {
    const crop = { x: 10, y: 20, width: 50, height: 50 };
    let state = beginGallerySession(idle, session({ crop, revision: 'rev-1' }));
    const outside = { ...mark('outside'), id: 'b', x: 9000 };
    state = updateGallerySessionOriginal(state, [outside, mark('before')], { width: 100, height: 100 });
    const finish = finishGallerySession(state, [mark('edited')], crop, false);
    expect(finish.kind).toBe('save');
    if (finish.kind !== 'save') return;
    expect(finish.plan.method).toBe('markup');
    expect(finish.plan.session.revision).toBe('rev-1');
    expect(finish.plan.annotations).toContainEqual(outside);
    const conflicted = failGallerySave(finish.state, 'revision conflict');
    expect(conflicted).toMatchObject({
      saveState: 'failed',
      failure: 'revision conflict',
      session: { revision: 'rev-1' },
    });
    const retry = finishGallerySession(conflicted, [mark('edited')], crop, false);
    if (retry.kind === 'save') expect(retry.plan).toMatchObject({ method: 'markup', session: { revision: 'rev-1' } });
    const changedCrop = finishGallerySession(state, [mark('before')], undefined, true);
    expect(changedCrop.kind).toBe('save');
    if (changedCrop.kind === 'save') expect(changedCrop.plan).toMatchObject({ method: 'markup', cropChanged: true });
  });

  it('discards a failed edit and restores the pending target, then accepts a new session', () => {
    let state = beginGallerySession(idle, session());
    state = queueGalleryReset(failGallerySave(state, 'revision conflict'), { url: 'other' });
    expect(state.failure).toBe('revision conflict');
    const discarded = discardGallerySession(state, 'current');
    expect(discarded).toEqual({ state: idle, reset: { url: 'other' } });
    state = beginGallerySession(discarded.state, session({ attachmentId: 'second' }));
    expect(state.session?.attachmentId).toBe('second');
    expect(state.pendingReset).toBeUndefined();
  });
});
