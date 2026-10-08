import { describe, expect, it } from 'vitest';

import type { MediaAnnotation } from './api';
import {
  croppableImageHeader,
  galleryCropFromDrag,
  projectGalleryAnnotations,
  restoreGalleryAnnotations,
} from './gallery-crop';

const original: MediaAnnotation[] = [
  { id: 'inside', x: 3000, y: 3000, width: 1000, height: 1000, text: 'inside' },
  { id: 'edge', x: 1000, y: 4000, width: 2000, height: 1000, text: 'edge' },
  { id: 'outside', x: 8000, y: 8000, width: 1000, height: 1000, text: 'outside' },
];
const crop = { x: 200, y: 100, width: 400, height: 400 };

describe('gallery crop coordinates', () => {
  it('matches the renderer format and animation gates', () => {
    const png = [137, 80, 78, 71, 13, 10, 26, 10],
      ascii = (value: string) => Array.from(new TextEncoder().encode(value)),
      chunk = (kind: string) => [0, 0, 0, 0, ...ascii(kind), 0, 0, 0, 0],
      webp = (kind: string, flag = 0) => [
        ...ascii('RIFF'),
        0,
        0,
        0,
        0,
        ...ascii('WEBP'),
        ...ascii(kind),
        0,
        0,
        0,
        0,
        flag,
      ];
    expect(croppableImageHeader('still.png', Uint8Array.from([...png, ...chunk('IDAT')]))).toBe(true);
    expect(croppableImageHeader('moving.png', Uint8Array.from([...png, ...chunk('acTL'), ...chunk('IDAT')]))).toBe(
      true,
    );
    expect(croppableImageHeader('still.jpg', Uint8Array.from([0xff, 0xd8, 0xff]))).toBe(true);
    expect(croppableImageHeader('still.webp', Uint8Array.from(webp('VP8X')))).toBe(true);
    expect(croppableImageHeader('moving.webp', Uint8Array.from(webp('VP8X', 0x02)))).toBe(true);
    expect(croppableImageHeader('moving.webp', Uint8Array.from(webp('ANIM')))).toBe(true);
    expect(croppableImageHeader('moving.gif', new TextEncoder().encode('GIF89a........'))).toBe(true);
    expect(croppableImageHeader('still.bmp', Uint8Array.from([...ascii('BM'), ...Array(24).fill(0)]))).toBe(true);
    expect(croppableImageHeader('icon.ico', Uint8Array.from([0, 0, 1, 0, 1, 0]))).toBe(true);
    expect(
      croppableImageHeader(
        'vector.svg',
        new TextEncoder().encode('<svg width="20" height="16" viewBox="0 0 20 16"><animate/></svg>'),
      ),
    ).toBe(true);
    expect(croppableImageHeader('vector.svg', new TextEncoder().encode('<svg width="100%" height="16"></svg>'))).toBe(
      false,
    );
    const avif = Uint8Array.from([0, 0, 0, 20, ...ascii('ftypavif'), 0, 0, 0, 0, ...ascii('mif1')]);
    expect(croppableImageHeader('still.avif', avif)).toBe(true);
    const animatedAvif = Uint8Array.from([...avif, ...ascii('avis')]);
    animatedAvif[3] = 24;
    expect(croppableImageHeader('animated.avif', animatedAvif)).toBe(false);
  });
  it('projects visible marks and preserves hidden originals across edits and restoration', () => {
    const projected = projectGalleryAnnotations(original, crop, 1000, 1000);
    expect(projected.map((item) => item.id)).toEqual(['inside', 'edge']);
    expect(projected[0]).toMatchObject({ x: 2500, y: 5000, width: 2500, height: 2500 });
    expect(projected[1]).toMatchObject({ x: 0, width: 2500 });
    projected[0].text = 'edited';
    const merged = restoreGalleryAnnotations(original, projected, crop, 1000, 1000);
    expect(merged.map((item) => item.id)).toEqual(['inside', 'edge', 'outside']);
    expect(merged.find((item) => item.id === 'outside')).toEqual(original[2]);
    expect(merged.find((item) => item.id === 'inside')).toMatchObject({
      x: 3000,
      y: 3000,
      width: 1000,
      height: 1000,
      text: 'edited',
    });
    expect(projectGalleryAnnotations(merged, undefined, 1000, 1000)).toHaveLength(3);
    expect(merged.find((item) => item.id === 'edge')).toEqual(original[1]);
    expect(merged.flatMap((item) => [item.x, item.y, item.width, item.height]).every(Number.isInteger)).toBe(true);
  });

  it('maps shape points through the same reversible coordinates', () => {
    const arrow: MediaAnnotation = {
      id: 'arrow',
      x: 3000,
      y: 3000,
      width: 1000,
      height: 1000,
      text: '',
      shape: {
        type: 'arrow',
        points: [
          { x: 3000, y: 4000 },
          { x: 4000, y: 3000 },
        ],
      },
    };
    const displayed = projectGalleryAnnotations([arrow], crop, 1000, 1000);
    expect(displayed[0].shape).toEqual({
      type: 'arrow',
      points: [
        { x: 2500, y: 7500 },
        { x: 5000, y: 5000 },
      ],
    });
    expect(restoreGalleryAnnotations([arrow], displayed, crop, 1000, 1000)[0]).toEqual(arrow);
  });

  it('keeps path bounding boxes exact after a partial crop and edit', () => {
    const paths: MediaAnnotation[] = [
      {
        id: 'arrow',
        x: 1000,
        y: 3000,
        width: 3000,
        height: 2000,
        text: '',
        shape: {
          type: 'arrow',
          points: [
            { x: 1000, y: 3000 },
            { x: 4000, y: 5000 },
          ],
        },
      },
      {
        id: 'freehand',
        x: 1000,
        y: 3000,
        width: 3000,
        height: 2000,
        text: '',
        shape: {
          type: 'freehand',
          points: [
            { x: 1000, y: 3000 },
            { x: 2500, y: 5000 },
            { x: 4000, y: 3000 },
          ],
        },
      },
    ];
    const projected = projectGalleryAnnotations(paths, crop, 1000, 1000);
    projected[0].text = 'edited';
    projected[1].text = 'edited';
    const saved = restoreGalleryAnnotations(paths, projected, crop, 1000, 1000);
    for (const item of saved) {
      if (item.shape?.type !== 'arrow' && item.shape?.type !== 'freehand') throw new Error('expected path');
      const xs = item.shape.points.map((point) => point.x),
        ys = item.shape.points.map((point) => point.y),
        x = Math.min(...xs),
        y = Math.min(...ys),
        maxX = Math.max(...xs),
        maxY = Math.max(...ys);
      expect(item).toMatchObject({
        x: Math.min(x, 9999),
        y: Math.min(y, 9999),
        width: maxX - Math.min(x, 9999) || 1,
        height: maxY - Math.min(y, 9999) || 1,
      });
      expect([...xs, ...ys, item.x, item.y, item.width, item.height].every(Number.isInteger)).toBe(true);
    }
  });

  it('rounds edited insertion geometry to the exact point bounds required by the server', () => {
    const insertion: MediaAnnotation = {
      id: 'insertion',
      x: 2000,
      y: 3500,
      width: 1,
      height: 1,
      text: '',
      shape: { type: 'insertion', point: { x: 2000, y: 3500 } },
    };
    const displayed = projectGalleryAnnotations([insertion], crop, 1000, 1000);
    expect(displayed[0].x).toBe(0);
    displayed[0].text = 'edited';
    const restored = restoreGalleryAnnotations([insertion], displayed, crop, 1000, 1000)[0];
    expect([restored.x, restored.y, restored.width, restored.height].every(Number.isInteger)).toBe(true);
    expect(restored.shape?.type === 'insertion' && Number.isInteger(restored.shape.point.x)).toBe(true);
    if (restored.shape?.type === 'insertion')
      expect(restored).toMatchObject({
        x: restored.shape.point.x,
        y: restored.shape.point.y,
        width: 1,
        height: 1,
      });
  });

  it('rejects sub-eight-pixel drag and rounds the selection into original pixels', () => {
    expect(galleryCropFromDrag({ x: 1000, y: 2000 }, { x: 9000, y: 8000 }, 1000, 500)).toEqual({
      x: 100,
      y: 100,
      width: 800,
      height: 300,
    });
    expect(galleryCropFromDrag({ x: 1000, y: 2000 }, { x: 1050, y: 2100 }, 1000, 500)).toBeUndefined();
  });
});
