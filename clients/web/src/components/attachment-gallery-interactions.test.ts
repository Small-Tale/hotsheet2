import { describe, expect, it, vi } from 'vitest';

import {
  type AttachmentGalleryControlBindings,
  wireAttachmentGalleryControls,
  wireGalleryVolumeSurface,
} from './attachment-gallery-interactions';

interface Registration {
  event: string;
  selector: string;
  handle: (event: unknown, target: unknown) => unknown;
}
const registrations = vi.hoisted(() => [] as Registration[]);
vi.mock('kerfjs', async (original) => ({
  ...(await original<typeof import('kerfjs')>()),
  delegate: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event, selector, handle });
    return () => undefined;
  },
  delegateCapture: (_root: unknown, event: string, selector: string, handle: Registration['handle']) => {
    registrations.push({ event: `capture:${event}`, selector, handle });
    return () => undefined;
  },
}));
const surfaces = vi.hoisted(
  () =>
    [] as {
      anchor: unknown;
      options: { tone?: string; label: string; onDismiss?: () => void };
      close: ReturnType<typeof vi.fn>;
      el: object;
    }[],
);
vi.mock('@kerfjs/ui/anchored-surface', () => ({
  openAnchoredSurface: (anchor: unknown, _content: unknown, options: (typeof surfaces)[number]['options']) => {
    const surface = { anchor, options, close: vi.fn(), el: {} };
    surfaces.push(surface);
    return surface;
  },
}));
const handler = (event: string, selector: string) =>
  registrations.find((item) => item.event === event && item.selector.includes(selector))!.handle;
const element = (dataset: Record<string, string> = {}, extra: Record<string, unknown> = {}) => ({
  dataset,
  matches: () => false,
  closest: () => null,
  ...extra,
});
const keyEvent = (key: string, origin: unknown = element(), shiftKey = false) => ({
  key,
  shiftKey,
  target: origin,
  preventDefault: vi.fn(),
  stopPropagation: vi.fn(),
});
const pointer = (type: string, pointerId: number, clientX = 0) =>
  Object.assign(new Event(type, { cancelable: true }), { pointerId, clientX });

describe('wireAttachmentGalleryControls (HS2-1KM6MB)', () => {
  const ownerDocument = new EventTarget();
  vi.stubGlobal('document', ownerDocument);
  let video: unknown = {};
  const root = { querySelector: () => video } as unknown as HTMLElement;
  const bindings = {
    close: vi.fn(),
    shift: vi.fn(),
    zoom: vi.fn(),
    toggleMarkup: vi.fn(),
    toggleDraw: vi.fn(),
    togglePlayback: vi.fn(),
    playback: () => ({ playheadMs: 1000, durationMs: 6000 }),
    seek: vi.fn(),
    seekAnnotation: vi.fn(),
    setVolume: vi.fn(),
    toggleVolume: vi.fn(),
    toggleMuted: vi.fn(),
    beginRange: vi.fn((annotationId: string | undefined) => annotationId === 'selected'),
    dragRange: vi.fn(),
    finishRange: vi.fn(),
    nudgeRange: vi.fn(),
  } satisfies AttachmentGalleryControlBindings;
  const dispose = wireAttachmentGalleryControls(root, bindings);

  it('routes navigation, zoom, markup, draw, playback, and annotation seeks', () => {
    handler('click', 'close-attachment-gallery')({}, element());
    handler('click', 'previous-gallery-image')({}, element());
    handler('click', 'next-gallery-image')({}, element());
    handler('click', 'zoom-gallery-image')({}, element({ zoomDirection: 'out' }));
    handler('click', 'zoom-gallery-image')({}, element());
    handler('click', 'toggle-gallery-markup')({}, element());
    handler('click', 'toggle-gallery-draw')({}, element());
    handler('click', 'toggle-gallery-playback')({}, element());
    handler('click', 'seek-gallery-annotation')({}, element({ annotationId: 'a', annotationTime: '1500' }));
    expect(bindings.close).toHaveBeenCalledTimes(1);
    expect(bindings.shift.mock.calls).toEqual([[-1], [1]]);
    expect(bindings.zoom.mock.calls).toEqual([['out'], ['in']]);
    expect(bindings.toggleMarkup).toHaveBeenCalledTimes(1);
    expect(bindings.toggleDraw).toHaveBeenCalledTimes(1);
    expect(bindings.togglePlayback).toHaveBeenCalledTimes(1);
    expect(bindings.seekAnnotation).toHaveBeenCalledWith('a', 1500);
  });

  it('maps playback keys only for a video gallery and not while another control has focus', () => {
    bindings.togglePlayback.mockClear();
    const keydown = handler('keydown', 'attachment-gallery');
    const space = keyEvent(' ');
    keydown(space, root);
    expect(bindings.togglePlayback).toHaveBeenCalledTimes(1);
    expect(space.stopPropagation).toHaveBeenCalled();
    keydown(keyEvent('ArrowRight'), root);
    expect(bindings.seek).toHaveBeenLastCalledWith(expect.any(Number), 'keyboard');
    bindings.seek.mockClear();
    keydown(keyEvent('ArrowRight', element({}, { closest: () => ({}) })), root);
    keydown(keyEvent('q'), root);
    video = null;
    keydown(keyEvent(' '), root);
    video = {};
    expect(bindings.seek).not.toHaveBeenCalled();
    expect(bindings.togglePlayback).toHaveBeenCalledTimes(1);
    const playhead = element({}, { matches: () => true, closest: () => ({}) });
    keydown(keyEvent('ArrowLeft', playhead), root);
    expect(bindings.seek).toHaveBeenCalledWith(expect.any(Number), 'keyboard');
  });

  it('distinguishes scrubbing from committed playhead and volume changes, clamping the volume', () => {
    handler('input', 'gallery-playhead')({}, { value: '2500' });
    handler('change', 'gallery-playhead')({}, { value: '3000' });
    handler('input', 'gallery-volume')({}, { value: '1.4' });
    handler('change', 'gallery-volume')({}, { value: '-1' });
    expect(bindings.seek.mock.calls.slice(-2)).toEqual([
      [2500, 'scrub'],
      [3000, 'commit'],
    ]);
    expect(bindings.setVolume.mock.calls).toEqual([
      [1, false],
      [0, true],
    ]);
    const stop = { stopPropagation: vi.fn() };
    handler('click', 'toggle-gallery-volume')(stop, element());
    handler('click', 'toggle-gallery-muted')(stop, element());
    expect(stop.stopPropagation).toHaveBeenCalledTimes(2);
    expect(bindings.toggleVolume).toHaveBeenCalledTimes(1);
    expect(bindings.toggleMuted).toHaveBeenCalledTimes(1);
  });

  it('nudges range endpoints from arrow keys and ignores other keys or handles', () => {
    const keydown = handler('keydown', 'gallery-range-handle');
    keydown(keyEvent('ArrowLeft'), element({ galleryRangeHandle: 'start', annotationId: 'selected' }));
    keydown(keyEvent('ArrowRight'), element({ galleryRangeHandle: 'end', annotationId: 'selected' }));
    keydown(keyEvent('Enter'), element({ galleryRangeHandle: 'end' }));
    keydown(keyEvent('ArrowRight'), element({ galleryRangeHandle: 'middle' }));
    expect(bindings.nudgeRange.mock.calls).toEqual([
      ['selected', 'start', -100],
      ['selected', 'end', 100],
    ]);
  });

  it('walks a range drag through begin, move, finish, cancel, foreign pointers, and refusal', () => {
    const pointerdown = handler('capture:pointerdown', 'gallery-range-handle');
    const handle = (annotationId: string, endpoint = 'end', track = true) =>
      element(
        { galleryRangeHandle: endpoint, annotationId },
        { closest: () => (track ? { getBoundingClientRect: () => ({ left: 100, width: 200 }) } : null) },
      );
    const down = { pointerId: 1, preventDefault: vi.fn(), stopPropagation: vi.fn() };
    // A refused or trackless handle never starts a drag.
    pointerdown(down, handle('other'));
    pointerdown(down, handle('selected', 'end', false));
    pointerdown(down, handle('selected', 'middle'));
    ownerDocument.dispatchEvent(pointer('pointermove', 1, 200));
    expect(bindings.dragRange).not.toHaveBeenCalled();
    pointerdown(down, handle('selected'));
    expect(down.preventDefault).toHaveBeenCalledTimes(1);
    ownerDocument.dispatchEvent(pointer('pointermove', 2, 200));
    ownerDocument.dispatchEvent(pointer('pointermove', 1, 200));
    expect(bindings.dragRange).toHaveBeenCalledWith('selected', 'end', 0.5);
    ownerDocument.dispatchEvent(pointer('pointerup', 2));
    expect(bindings.finishRange).not.toHaveBeenCalled();
    ownerDocument.dispatchEvent(pointer('pointerup', 1));
    expect(bindings.finishRange).toHaveBeenLastCalledWith(false);
    // A repeated release after the drag ended is ignored; a fresh drag can be cancelled.
    ownerDocument.dispatchEvent(pointer('pointerup', 1));
    pointerdown(down, handle('selected', 'start'));
    ownerDocument.dispatchEvent(pointer('pointercancel', 1));
    expect(bindings.finishRange.mock.calls).toEqual([[false], [true]]);
  });

  it('stops listening to document pointer events once disposed', () => {
    const pointerdown = handler('capture:pointerdown', 'gallery-range-handle');
    pointerdown(
      { pointerId: 3, preventDefault: vi.fn(), stopPropagation: vi.fn() },
      element(
        { galleryRangeHandle: 'start', annotationId: 'selected' },
        { closest: () => ({ getBoundingClientRect: () => ({ left: 0, width: 100 }) }) },
      ),
    );
    dispose();
    bindings.dragRange.mockClear();
    ownerDocument.dispatchEvent(pointer('pointermove', 3, 50));
    expect(bindings.dragRange).not.toHaveBeenCalled();
  });
});

describe('wireGalleryVolumeSurface (HS2-HTRFDS)', () => {
  it('opens one dark Kerf surface per open, closes on toggle/dispose, and syncs user dismissal', async () => {
    const { signal } = await import('kerfjs');
    const open = signal(false),
      trigger = { id: 'volume' },
      root = { querySelector: vi.fn(() => trigger) } as unknown as ParentNode,
      bindings = { setVolume: vi.fn(), toggleMuted: vi.fn() };
    surfaces.length = 0;
    const dispose = wireGalleryVolumeSurface(root, open, () => '' as never, bindings);
    expect(surfaces).toHaveLength(0);
    open.value = true;
    expect(surfaces).toHaveLength(1);
    expect(surfaces[0].anchor).toBe(trigger);
    expect(surfaces[0].options).toMatchObject({ tone: 'dark', label: 'Volume Controls' });
    // Repeated true is not a second surface; false closes it; refill opens a fresh one.
    open.value = true;
    expect(surfaces).toHaveLength(1);
    open.value = false;
    expect(surfaces[0].close).toHaveBeenCalledTimes(1);
    open.value = true;
    expect(surfaces).toHaveLength(2);
    // Escape/outside dismissal clears the open state without a second close.
    surfaces[1].options.onDismiss?.();
    expect(open.value).toBe(false);
    expect(surfaces[1].close).not.toHaveBeenCalled();
    // The slider and mute row on the surface reach the bindings and stop at the surface.
    open.value = true;
    const stop = vi.fn(),
      latest = (event: string, selector: string) =>
        [...registrations].reverse().find((item) => item.event === event && item.selector.includes(selector))!.handle;
    latest('input', 'gallery-volume')({ stopPropagation: stop }, { value: '0.3' });
    latest('change', 'gallery-volume')({ stopPropagation: stop }, { value: '2' });
    latest('click', 'toggle-gallery-muted')({ stopPropagation: stop }, {});
    expect(bindings.setVolume.mock.calls).toEqual([
      [0.3, false],
      [1, true],
    ]);
    expect(bindings.toggleMuted).toHaveBeenCalledTimes(1);
    expect(stop).toHaveBeenCalledTimes(3);
    dispose();
    expect(surfaces[2].close).toHaveBeenCalledTimes(1);
    // A missing trigger opens nothing.
    const empty = signal(true);
    surfaces.length = 0;
    wireGalleryVolumeSurface(
      { querySelector: () => null } as unknown as ParentNode,
      empty,
      () => '' as never,
      bindings,
    )();
    expect(surfaces).toHaveLength(0);
  });
});
