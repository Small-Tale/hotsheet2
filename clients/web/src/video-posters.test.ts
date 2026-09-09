import { afterEach, describe, expect, it, vi } from 'vitest';

import { ensureVideoPoster } from './video-posters';

afterEach(() => vi.restoreAllMocks());

describe('portable video posters', () => {
  it('short-circuits generation when the content-addressed poster exists', async () => {
    const poster = new Blob(['existing'], { type: 'image/jpeg' });
    const fetcher = vi.fn<typeof fetch>().mockResolvedValue(new Response(poster));
    const source = vi.fn<() => Promise<Blob>>();
    const generator = vi.fn<(source: Blob) => Promise<Blob>>();
    await expect(ensureVideoPoster('/thumbnail/existing', source, fetcher, generator)).resolves.toMatchObject({ state: 'existing' });
    expect(source).not.toHaveBeenCalled();
    expect(generator).not.toHaveBeenCalled();
    expect(fetcher).toHaveBeenCalledTimes(1);
  });

  it('coalesces concurrent misses and uploads one browser-generated JPEG', async () => {
    const generated = new Blob(['generated'], { type: 'image/jpeg' });
    const fetcher = vi.fn<typeof fetch>()
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
    expect(fetcher).toHaveBeenLastCalledWith('/thumbnail/shared', expect.objectContaining({ method: 'PUT', body: generated }));
  });

  it.each([
    ['poster GET failure', vi.fn<typeof fetch>().mockResolvedValue(new Response(null, { status: 500 }))],
    ['unsupported source codec', vi.fn<typeof fetch>().mockResolvedValueOnce(new Response(null, { status: 404 }))],
    ['poster PUT failure', vi.fn<typeof fetch>().mockResolvedValueOnce(new Response(null, { status: 404 })).mockResolvedValueOnce(new Response(null, { status: 503 }))],
  ])('falls back without throwing on %s', async (kind, fetcher) => {
    const generator = kind === 'unsupported source codec'
      ? vi.fn<(source: Blob) => Promise<Blob>>().mockRejectedValue(new Error('codec'))
      : vi.fn(async () => new Blob(['poster'], { type: 'image/jpeg' }));
    await expect(ensureVideoPoster(`/thumbnail/${kind}`, new Blob(['video']), fetcher, generator)).resolves.toEqual({ state: 'unavailable' });
  });
});
