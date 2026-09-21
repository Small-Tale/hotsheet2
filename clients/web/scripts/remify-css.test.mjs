import postcss from 'postcss';
import { describe, expect, it } from 'vitest';

import remifyCss, { transformRemifyValue } from './remify-css.mjs';

describe('remify CSS authoring transform', () => {
  it('converts integer, fractional, negative, and zero pixel literals exactly', () => {
    expect(transformRemifyValue('remify(17px) remify(18px) remify(40.4px) remify(-2px) remify(0px)')).toBe(
      '1.0625rem 1.125rem 2.525rem -0.125rem 0rem',
    );
  });

  it('converts declarations and at-rule parameters through PostCSS', async () => {
    const result = await postcss([remifyCss()]).process(
      `
      .sample { padding: remify(17px); width: calc(100% - remify(32px)); }
      @media (max-width: remify(768px)) { .sample { gap: remify(.5px); } }
    `,
      { from: 'fixture.css' },
    );

    expect(result.css).toContain('padding: 1.0625rem');
    expect(result.css).toContain('width: calc(100% - 2rem)');
    expect(result.css).toContain('@media (max-width: 48rem)');
    expect(result.css).toContain('gap: 0.03125rem');
  });

  it('does not transform quoted or commented examples', () => {
    expect(transformRemifyValue('"remify(16px)" /* remify(32px) */ remify(48px)')).toBe(
      '"remify(16px)" /* remify(32px) */ 3rem',
    );
  });

  it('rejects expressions and non-pixel arguments with a source location', async () => {
    await expect(
      postcss([remifyCss()]).process('.bad { width: remify(var(--size)); }', { from: 'fixture.css' }),
    ).rejects.toThrow('remify() accepts one numeric px literal');
    await expect(
      postcss([remifyCss()]).process('.bad { width: remify(2em); }', { from: 'fixture.css' }),
    ).rejects.toThrow('fixture.css:1:8');
  });
});
