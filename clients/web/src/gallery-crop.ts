import type { Attachment, MediaAnnotation } from './api';

export type ImageCrop = NonNullable<Attachment['crop']>;

/** Recognize crop-capable image bytes before offering an editor the server would reject. */
export function croppableImageHeader(filename: string, bytes: Uint8Array): boolean {
  const name = filename.toLowerCase();
  if (/\.gif$/.test(name))
    return bytes.length >= 10 && (ascii(bytes, 0, 6) === 'GIF87a' || ascii(bytes, 0, 6) === 'GIF89a');
  if (/\.bmp$/.test(name)) return bytes.length >= 26 && ascii(bytes, 0, 2) === 'BM';
  if (/\.ico$/.test(name)) return bytes.length >= 6 && [0, 0, 1, 0].every((value, index) => bytes[index] === value);
  if (/\.avif$/.test(name)) {
    if (bytes.length < 16 || ascii(bytes, 4, 8) !== 'ftyp') return false;
    const size = Math.min(new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).getUint32(0), bytes.length);
    const brands = Array.from({ length: Math.floor((size - 8) / 4) }, (_, index) =>
      ascii(bytes, 8 + index * 4, 12 + index * 4),
    );
    return brands.includes('avif') && !brands.includes('avis') && !brands.includes('msf1');
  }
  if (/\.svg$/.test(name)) {
    const text = new TextDecoder().decode(bytes.subarray(0, 8192));
    if (/<!DOCTYPE/i.test(text)) return false;
    const root = text.match(/^\s*(?:<\?xml[^>]*>\s*)?(?:<!--[\s\S]*?-->\s*)*<svg\b([^>]*)>/i)?.[1];
    if (!root) return false;
    const dimension = (attribute: string) => {
      const value = root.match(new RegExp(`\\b${attribute}=["']([0-9]+)(?:px)?["']`, 'i'))?.[1];
      return value ? Number(value) : 0;
    };
    const width = dimension('width'),
      height = dimension('height');
    if (!width || !height || width * height > 40_000_000) return false;
    const viewBox = root
      .match(/\bviewBox=["']([^"']+)["']/i)?.[1]
      ?.split(/[\s,]+/)
      .map(Number);
    return !viewBox || (viewBox.length === 4 && viewBox.every(Number.isFinite) && viewBox[2] > 0 && viewBox[3] > 0);
  }
  if (/\.jpe?g$/.test(name)) return bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff;
  if (/\.png$/.test(name)) {
    if (bytes.length < 8 || ![137, 80, 78, 71, 13, 10, 26, 10].every((value, index) => bytes[index] === value))
      return false;
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    for (let offset = 8; offset + 12 <= bytes.length;) {
      const length = view.getUint32(offset),
        kind = String.fromCharCode(...bytes.subarray(offset + 4, offset + 8));
      if (kind === 'acTL') return true;
      if (kind === 'IDAT') return true;
      offset += 12 + length;
    }
    return false;
  }
  if (/\.webp$/.test(name)) {
    if (
      bytes.length < 20 ||
      String.fromCharCode(...bytes.subarray(0, 4)) !== 'RIFF' ||
      String.fromCharCode(...bytes.subarray(8, 12)) !== 'WEBP'
    )
      return false;
    const kind = String.fromCharCode(...bytes.subarray(12, 16));
    if (kind === 'ANIM') return true;
    if (kind === 'VP8X') return bytes.length >= 21;
    return kind === 'VP8 ' || kind === 'VP8L';
  }
  return false;
}

function ascii(bytes: Uint8Array, start: number, end: number): string {
  return String.fromCharCode(...bytes.subarray(start, end));
}

/** Annotation coordinates are ten-thousandths of the immutable original image. */
export function projectGalleryAnnotations(
  annotations: readonly MediaAnnotation[],
  crop: ImageCrop | undefined,
  originalWidth: number,
  originalHeight: number,
): MediaAnnotation[] {
  if (!crop || !originalWidth || !originalHeight) return annotations.map((item) => structuredClone(item));
  const x0 = (crop.x * 10_000) / originalWidth,
    y0 = (crop.y * 10_000) / originalHeight,
    sx = originalWidth / crop.width,
    sy = originalHeight / crop.height;
  return annotations
    .filter(
      (item) =>
        item.x < x0 + 10_000 / sx && item.x + item.width > x0 && item.y < y0 + 10_000 / sy && item.y + item.height > y0,
    )
    .map((item) => mapAnnotation(item, (point) => ({ x: (point.x - x0) * sx, y: (point.y - y0) * sy }), sx, sy, true));
}

/** Merge visible edits back into original coordinates without losing marks outside the crop. */
export function restoreGalleryAnnotations(
  original: readonly MediaAnnotation[],
  displayed: readonly MediaAnnotation[],
  crop: ImageCrop | undefined,
  originalWidth: number,
  originalHeight: number,
): MediaAnnotation[] {
  if (!crop || !originalWidth || !originalHeight) return displayed.map((item) => structuredClone(item));
  const visible = projectGalleryAnnotations(original, crop, originalWidth, originalHeight),
    visibleById = new Map(visible.map((item) => [item.id, item])),
    displayedById = new Map(displayed.map((item) => [item.id, item])),
    originalIds = new Set(original.map((item) => item.id)),
    x0 = (crop.x * 10_000) / originalWidth,
    y0 = (crop.y * 10_000) / originalHeight,
    sx = crop.width / originalWidth,
    sy = crop.height / originalHeight;
  const inverse = (item: MediaAnnotation) =>
    mapAnnotation(item, (point) => ({ x: x0 + point.x * sx, y: y0 + point.y * sy }), sx, sy, false);
  const result: MediaAnnotation[] = [];
  for (const canonical of original) {
    const projected = visibleById.get(canonical.id);
    if (!projected) {
      result.push(structuredClone(canonical));
      continue;
    }
    const edited = displayedById.get(canonical.id);
    if (!edited) continue;
    result.push(JSON.stringify(projected) === JSON.stringify(edited) ? structuredClone(canonical) : inverse(edited));
  }
  for (const item of displayed) if (!originalIds.has(item.id)) result.push(inverse(item));
  return result;
}

function mapAnnotation(
  item: MediaAnnotation,
  point: (point: { x: number; y: number }) => { x: number; y: number },
  sx: number,
  sy: number,
  clip: boolean,
): MediaAnnotation {
  const clamp = (value: number) => Math.max(0, Math.min(10_000, Math.round(value))),
    mapped = (value: { x: number; y: number }) => {
      const result = point(value);
      return { x: clamp(result.x), y: clamp(result.y) };
    },
    origin = point(item),
    right = origin.x + item.width * sx,
    bottom = origin.y + item.height * sy,
    x = clip ? clamp(origin.x) : Math.round(origin.x),
    y = clip ? clamp(origin.y) : Math.round(origin.y),
    width = Math.max(1, (clip ? clamp(right) : Math.round(right)) - x),
    height = Math.max(1, (clip ? clamp(bottom) : Math.round(bottom)) - y),
    shape = item.shape,
    mappedShape =
      shape?.type === 'freehand' || shape?.type === 'arrow'
        ? { ...shape, points: shape.points.map(mapped) }
        : shape?.type === 'insertion'
          ? { ...shape, point: mapped(shape.point) }
          : shape,
    points =
      mappedShape?.type === 'freehand' || mappedShape?.type === 'arrow'
        ? mappedShape.points
        : mappedShape?.type === 'insertion'
          ? [mappedShape.point]
          : undefined,
    pointBounds = points?.length
      ? (() => {
          const minX = Math.min(...points.map((value) => value.x)),
            minY = Math.min(...points.map((value) => value.y)),
            maxX = Math.max(...points.map((value) => value.x)),
            maxY = Math.max(...points.map((value) => value.y)),
            px = Math.min(minX, 9999),
            py = Math.min(minY, 9999);
          return { x: px, y: py, width: Math.max(maxX, px + 1) - px, height: Math.max(maxY, py + 1) - py };
        })()
      : undefined;
  return {
    ...structuredClone(item),
    x: pointBounds?.x ?? Math.min(x, 9999),
    y: pointBounds?.y ?? Math.min(y, 9999),
    width: pointBounds?.width ?? Math.min(width, 10000 - Math.min(x, 9999)),
    height: pointBounds?.height ?? Math.min(height, 10000 - Math.min(y, 9999)),
    shape: mappedShape,
  };
}

export function galleryCropFromDrag(
  start: { x: number; y: number },
  end: { x: number; y: number },
  width: number,
  height: number,
): ImageCrop | undefined {
  if (!width || !height) return;
  const x = Math.round((Math.min(start.x, end.x) * width) / 10_000),
    y = Math.round((Math.min(start.y, end.y) * height) / 10_000),
    right = Math.round((Math.max(start.x, end.x) * width) / 10_000),
    bottom = Math.round((Math.max(start.y, end.y) * height) / 10_000);
  return right - x >= 8 && bottom - y >= 8 ? { x, y, width: right - x, height: bottom - y } : undefined;
}
