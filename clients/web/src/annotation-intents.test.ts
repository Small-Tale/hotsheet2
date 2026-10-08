import { describe, expect, it } from 'vitest';

import { annotationDefaultIntent, annotationIntentColor, annotationPrimaryIntent } from './annotation-intents';
import type { MediaAnnotation } from './api';

const annotation = (shape?: MediaAnnotation['shape'], intents?: string[]): MediaAnnotation => ({
  id: 'mark',
  x: 100,
  y: 200,
  width: 300,
  height: 400,
  text: '',
  shape,
  intents,
});

describe('media annotation intents', () => {
  it('derives defaults from each shape and prioritizes the first non-default intent', () => {
    expect(annotationDefaultIntent(annotation())).toBe('comment');
    expect(annotationDefaultIntent(annotation({ type: 'rect' }))).toBe('comment');
    expect(annotationDefaultIntent(annotation({ type: 'freehand', points: [] }))).toBe('comment');
    expect(annotationDefaultIntent(annotation({ type: 'strike' }))).toBe('remove');
    expect(annotationDefaultIntent(annotation({ type: 'insertion', point: { x: 1, y: 2 } }))).toBe('insert');
    expect(annotationDefaultIntent(annotation({ type: 'arrow', points: [] }))).toBe('move');
    expect(annotationPrimaryIntent(annotation(undefined, ['comment', 'bug', 'question']))).toBe('bug');
    expect(annotationPrimaryIntent(annotation({ type: 'strike' }, []))).toBe('remove');
  });

  it('maps known intents to color families without discarding unknown values', () => {
    for (const [intent, color] of [
      ['comment', 'blue'],
      ['bug', 'red'],
      ['change', 'orange'],
      ['insert', 'green'],
      ['remove', 'purple'],
      ['move', 'teal'],
      ['question', 'yellow'],
    ]) {
      expect(annotationIntentColor(annotation(undefined, [intent]))).toBe(color);
    }
    const future = annotation(undefined, ['comment', 'future_focus']);
    expect(annotationPrimaryIntent(future)).toBe('future_focus');
    expect(annotationIntentColor(future)).toBeUndefined();
    expect(future.intents).toEqual(['comment', 'future_focus']);
  });
});
