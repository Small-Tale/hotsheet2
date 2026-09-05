import { describe, expect, it } from 'vitest';

import { replaceModernColorFunctions } from './capture-colors';

describe('replaceModernColorFunctions', () => {
  it('normalizes modern colors in standalone, shadow, and gradient values', () => {
    const seen: string[] = [];
    const convert = (color: string) => {
      seen.push(color);
      return 'rgb(1, 2, 3)';
    };

    expect(replaceModernColorFunctions('oklab(50% 0.1 0.2)', convert)).toBe('rgb(1, 2, 3)');
    expect(replaceModernColorFunctions('0 1px 2px oklch(70% 0.2 40 / 50%)', convert)).toBe('0 1px 2px rgb(1, 2, 3)');
    expect(replaceModernColorFunctions('linear-gradient(lab(20% 1 2), color(display-p3 1 0 0))', convert)).toBe('linear-gradient(rgb(1, 2, 3), rgb(1, 2, 3))');
    expect(seen).toEqual([
      'oklab(50% 0.1 0.2)',
      'oklch(70% 0.2 40 / 50%)',
      'lab(20% 1 2)',
      'color(display-p3 1 0 0)',
    ]);
  });

  it('leaves legacy colors and malformed functions untouched', () => {
    expect(replaceModernColorFunctions('rgb(1, 2, 3)', () => 'unexpected')).toBe('rgb(1, 2, 3)');
    expect(replaceModernColorFunctions('oklab(50% 0.1 0.2', () => 'unexpected')).toBe('oklab(50% 0.1 0.2');
  });
});
