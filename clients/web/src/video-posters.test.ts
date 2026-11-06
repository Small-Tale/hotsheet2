import { afterEach, describe, expect, it, vi } from 'vitest';

import { ensureVideoPoster, releaseVideoPreviewSource, syncVideoPosters } from './video-posters';

afterEach(() => vi.restoreAllMocks());

describe('portable video posters', () => {
  it('drops the preview source and resets the media pipeline after a poster is ready', () => {
    const video = { load: vi.fn(), removeAttribute: vi.fn() };
    releaseVideoPreviewSource(video);
    expect(video.removeAttribute).toHaveBeenCalledWith('src');
    expect(video.load).toHaveBeenCalledOnce();
  });

  it('short-circuits generation when the content-addressed poster exists', async () => {
    const poster = new Blob(['existing'], { type: 'image/jpeg' });
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(poster));
    const source = vi.fn<() => Promise<Blob>>();
    const generator = vi.fn<(source: Blob) => Promise<Blob>>();
    await expect(ensureVideoPoster('/thumbnail/existing', source, fetcher, generator)).resolves.toMatchObject({
      state: 'existing',
    });
    expect(source).not.toHaveBeenCalled();
    expect(generator).not.toHaveBeenCalled();
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it('coalesces concurrent misses and uploads one browser-generated JPEG', async () => {
    const generated = new Blob(['generated'], { type: 'image/jpeg' });
    const fetcher = vi
      .fn<typeof fetch>()
      .mockResolvedValueOnce(new Response(null, { status: 404 }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }));
    const generator = vi.fn(async () => generated);
    const first = ensureVideoPoster('/thumbnail/shared', new Blob(['video']), fetcher, generator);
    const second = ensureVideoPoster('/thumbnail/shared', new Blob(['video']), fetcher, generator);
    await expect(Promise.all([first, second])).resolves.toEqual([
      { state: 'generated', poster: generated },
      { state: 'generated', poster: generated },
    ]);
    expect(generator).toHaveBeenCalledTimes(1);
    expect(fetcher).toHaveBeenCalledTimes(2);
    expect(fetcher).toHaveBeenLastCalledWith(
      '/thumbnail/shared',
      expect.objectContaining({ method: 'PUT', body: generated }),
    );
  });

  it.each([
    ['poster GET failure', vi.fn<typeof fetch>().mockResolvedValue(new Response(null, { status: 500 }))],
    ['unsupported source codec', vi.fn<typeof fetch>().mockResolvedValueOnce(new Response(null, { status: 404 }))],
    [
      'poster PUT failure',
      vi
        .fn<typeof fetch>()
        .mockResolvedValueOnce(new Response(null, { status: 404 }))
        .mockResolvedValueOnce(new Response(null, { status: 503 })),
    ],
  ])('falls back without throwing on %s', async (kind, fetcher) => {
    const generator =
      kind === 'unsupported source codec'
        ? vi.fn<(source: Blob) => Promise<Blob>>().mockRejectedValue(new Error('codec'))
        : vi.fn(async () => new Blob(['poster'], { type: 'image/jpeg' }));
    await expect(ensureVideoPoster(`/thumbnail/${kind}`, new Blob(['video']), fetcher, generator)).resolves.toEqual({
      state: 'unavailable',
    });
  });

  it('HS2-E2YQMF restores a hydrated local poster after a re-render resets it to the server URL', async () => {
    const created = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:local-poster');
    vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => undefined);
    const attributes = new Map<string, string>([
      ['poster', '/attachments/V9/thumbnail'],
      ['src', '/attachments/V9#t=0.1'],
    ]);
    const video = {
      dataset: { videoPosterUrl: '/attachments/V9/thumbnail', videoSourceUrl: '/attachments/V9' },
      isConnected: true,
      getAttribute: (name: string) => attributes.get(name) ?? null,
      setAttribute: (name: string, value: string) => attributes.set(name, value),
      removeAttribute: (name: string) => attributes.delete(name),
      load: vi.fn(),
      set poster(value: string) {
        attributes.set('poster', value);
      },
      get poster() {
        return attributes.get('poster') ?? '';
      },
    };
    const root = { contains: () => true, querySelectorAll: () => [video] } as unknown as ParentNode;
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(new Blob(['poster'], { type: 'image/jpeg' })));
    syncVideoPosters(root, fetcher);
    await vi.waitFor(() => {
      expect(attributes.get('poster')).toBe('blob:local-poster');
    });
    expect(attributes.has('src')).toBe(false);
    // A later render morphs the rendered attributes back onto the same element.
    attributes.set('poster', '/attachments/V9/thumbnail');
    attributes.set('src', '/attachments/V9#t=0.1');
    syncVideoPosters(root, fetcher);
    expect(attributes.get('poster')).toBe('blob:local-poster');
    expect(attributes.has('src')).toBe(false);
    // Restoring reuses the hydrated poster: no second fetch and no second object URL.
    expect(fetcher).toHaveBeenCalledTimes(1);
    expect(created).toHaveBeenCalledTimes(1);
  });
});
