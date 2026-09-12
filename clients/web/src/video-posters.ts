export type VideoPosterState = 'existing' | 'generated' | 'unavailable';

export interface VideoPosterResult {
  state: VideoPosterState;
  poster?: Blob;
}

type Fetcher = typeof fetch;
type PosterGenerator = (source: Blob) => Promise<Blob>;

const posterTasks = new Map<string, Promise<VideoPosterResult>>();
const assignedPosters = new WeakMap<HTMLVideoElement, string>();
const localPosterUrls = new Map<HTMLVideoElement, string>();

function waitForMedia(video: HTMLVideoElement, event: 'loadeddata' | 'seeked'): Promise<void> {
  return new Promise((resolve, reject) => {
    const timeout = window.setTimeout(() => { finish(new Error('video poster generation timed out')); }, 8_000);
    const finish = (error?: Error) => {
      window.clearTimeout(timeout);
      video.removeEventListener(event, ready);
      video.removeEventListener('error', failed);
      if (error) reject(error);
      else resolve();
    };
    const ready = () => { finish(); };
    const failed = () => { finish(new Error('the browser cannot decode this video')); };
    video.addEventListener(event, ready, { once: true });
    video.addEventListener('error', failed, { once: true });
  });
}

function canvasJpeg(canvas: HTMLCanvasElement): Promise<Blob> {
  return new Promise((resolve, reject) => {
    canvas.toBlob(blob => {
      if (blob) resolve(blob);
      else reject(new Error('canvas could not encode a JPEG'));
    }, 'image/jpeg', 0.84);
  });
}

/** Generate a portable video poster using only browser-native media and canvas APIs. */
export async function generateVideoPoster(source: Blob): Promise<Blob> {
  const sourceUrl = URL.createObjectURL(source);
  const video = document.createElement('video');
  video.muted = true;
  video.preload = 'auto';
  video.playsInline = true;
  try {
    const loaded = waitForMedia(video, 'loadeddata');
    video.src = sourceUrl;
    video.load();
    await loaded;
    if (Number.isFinite(video.duration) && video.duration > 0.1) {
      const sought = waitForMedia(video, 'seeked');
      video.currentTime = Math.min(0.1, video.duration / 2);
      await sought;
    }
    if (!video.videoWidth || !video.videoHeight) throw new Error('video has no drawable frame');
    const scale = Math.min(1, 640 / video.videoWidth);
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(video.videoWidth * scale));
    canvas.height = Math.max(1, Math.round(video.videoHeight * scale));
    const context = canvas.getContext('2d');
    if (!context) throw new Error('2D canvas is unavailable');
    context.drawImage(video, 0, 0, canvas.width, canvas.height);
    return await canvasJpeg(canvas);
  } finally {
    video.removeAttribute('src');
    video.load();
    URL.revokeObjectURL(sourceUrl);
  }
}

/**
 * Reuse a server poster when present, otherwise generate and publish it. All failures
 * degrade to `unavailable`; poster work must never prevent the original video playing.
 */
export async function ensureVideoPoster(
  posterUrl: string,
  source: Blob | (() => Promise<Blob>),
  fetcher: Fetcher = fetch,
  generator: PosterGenerator = generateVideoPoster,
): Promise<VideoPosterResult> {
  const active = posterTasks.get(posterUrl);
  if (active) return active;
  const task = (async (): Promise<VideoPosterResult> => {
    try {
      const existing = await fetcher(posterUrl, { cache: 'no-store' });
      if (existing.ok) return { state: 'existing', poster: await existing.blob() };
      if (existing.status !== 404) return { state: 'unavailable' };
      const blob = typeof source === 'function' ? await source() : source;
      const poster = await generator(blob);
      const uploaded = await fetcher(posterUrl, {
        method: 'PUT',
        headers: { 'content-type': 'image/jpeg' },
        body: poster,
      });
      if (!uploaded.ok) return { state: 'unavailable' };
      return { state: 'generated', poster };
    } catch {
      return { state: 'unavailable' };
    }
  })();
  posterTasks.set(posterUrl, task);
  try {
    return await task;
  } finally {
    posterTasks.delete(posterUrl);
  }
}

async function sourceBlob(url: string, fetcher: Fetcher): Promise<Blob> {
  const response = await fetcher(url);
  if (!response.ok) throw new Error(`video request failed with ${response.status}`);
  return response.blob();
}

/** Keep poster-backed list previews from retaining a media element pipeline. */
export function releaseVideoPreviewSource(video: Pick<HTMLVideoElement, 'load' | 'removeAttribute'>): void {
  video.removeAttribute('src');
  video.load();
}

/** Lazily backfill missing posters for video previews rendered by the real client. */
export function syncVideoPosters(root: ParentNode, fetcher: Fetcher = fetch): void {
  for (const [video, url] of localPosterUrls) {
    if (!root.contains(video)) {
      URL.revokeObjectURL(url);
      localPosterUrls.delete(video);
    }
  }
  for (const video of root.querySelectorAll<HTMLVideoElement>('video[data-video-poster-url]')) {
    const posterUrl = video.dataset.videoPosterUrl;
    const videoUrl = video.dataset.videoSourceUrl;
    if (!posterUrl || !videoUrl) continue;
    if (assignedPosters.get(video) === posterUrl) {
      if (localPosterUrls.has(video)) releaseVideoPreviewSource(video);
      continue;
    }
    assignedPosters.set(video, posterUrl);
    void ensureVideoPoster(posterUrl, () => sourceBlob(videoUrl, fetcher), fetcher).then(result => {
      if (!result.poster || !video.isConnected) return;
      const previous = localPosterUrls.get(video);
      const localPoster = URL.createObjectURL(result.poster);
      localPosterUrls.set(video, localPoster);
      video.poster = localPoster;
      if (previous) URL.revokeObjectURL(previous);
      releaseVideoPreviewSource(video);
    });
  }
}
