import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { GlanceGauge, type GlanceGaugeProps } from './glance-gauge';

const TOKENS: GlanceGaugeProps = { value: 1600, peak: 6200, label: 'Token rate', unit: 'tokens/min' };

function render(props: Partial<GlanceGaugeProps> = {}): string {
  return String(GlanceGauge({ ...TOKENS, ...props }));
}

/** One attribute of the gauge's root `svg`. */
function rootAttribute(markup: string, name: string): string | undefined {
  const root = /^<svg\b[^>]*>/.exec(markup)?.[0] ?? '';
  return new RegExp(`\\s${name}="([^"]*)"`).exec(root)?.[1];
}

function count(markup: string, pattern: RegExp): number {
  return markup.match(new RegExp(pattern.source, 'g'))?.length ?? 0;
}

describe('GlanceGauge (HS2-CVE3SK)', () => {
  it('is one named image whose hover title is its accessible name', () => {
    const markup = render();
    expect(rootAttribute(markup, 'class')).toBe('glance-gauge');
    expect(rootAttribute(markup, 'role')).toBe('img');
    expect(rootAttribute(markup, 'aria-label')).toBe('Token rate: 1.6k tokens/min (30-day peak 6.2k)');
    expect(markup).toContain('<title>Token rate: 1.6k tokens/min (30-day peak 6.2k)</title>');
  });

  it('defaults to the full brand dial at the large size on the theme palette', () => {
    const markup = render();
    expect(rootAttribute(markup, 'data-variant')).toBe('full');
    expect(rootAttribute(markup, 'data-tone')).toBe('brand');
    expect(rootAttribute(markup, 'data-palette')).toBe('theme');
    expect(rootAttribute(markup, 'data-size')).toBe('large');
    expect(rootAttribute(markup, 'viewBox')).toBe('0 0 100 86');
  });

  it('draws ticks plus the value and unit inside the full dial', () => {
    const markup = render();
    expect(markup).toMatch(/<text class="glance-gauge__value"[^>]*>1\.6k<\/text>/);
    expect(markup).toMatch(/<text class="glance-gauge__unit"[^>]*>tokens\/min<\/text>/);
    expect(count(markup, /class="glance-gauge__tick"/)).toBe(11);
  });

  it('keeps the bare dial free of text and ticks, at chip size by default', () => {
    const markup = render({ variant: 'bare', tone: 'success', value: 14, peak: 22, unit: 'tickets/h' });
    expect(rootAttribute(markup, 'data-variant')).toBe('bare');
    expect(rootAttribute(markup, 'data-tone')).toBe('success');
    expect(rootAttribute(markup, 'data-size')).toBe('chip');
    expect(rootAttribute(markup, 'viewBox')).toBe('0 0 100 78');
    expect(markup).not.toContain('<text');
    expect(markup).not.toContain('glance-gauge__tick');
    expect(markup).toContain('class="glance-gauge__needle"');
    expect(rootAttribute(markup, 'aria-label')).toBe('Token rate: 14 tickets/h (30-day peak 22)');
  });

  it('rotates the needle and dashes the value arc to the reading', () => {
    const markup = render({ value: 1550 });
    expect(rootAttribute(markup, 'data-fraction')).toBe('0.25');
    expect(rootAttribute(markup, 'data-angle')).toBe('-60');
    expect(rootAttribute(markup, 'data-empty')).toBe('false');
    expect(rootAttribute(markup, 'style')).toBe('--_glance-gauge-angle:-60deg;--_glance-gauge-fill:25');
    expect(markup).toMatch(/class="glance-gauge__fill"[^>]*pathLength="100"/);
  });

  it('marks an idle reading empty and pins one above its peak', () => {
    const idle = render({ value: 0 });
    expect(rootAttribute(idle, 'data-empty')).toBe('true');
    expect(rootAttribute(idle, 'data-angle')).toBe('-120');
    const above = render({ value: 9000 });
    expect(rootAttribute(above, 'data-fraction')).toBe('1');
    expect(rootAttribute(above, 'data-angle')).toBe('120');
  });

  it('carries the standby palette and an explicit size', () => {
    const standby = render({ palette: 'standby', size: 'fill' });
    expect(rootAttribute(standby, 'data-palette')).toBe('standby');
    expect(rootAttribute(standby, 'data-size')).toBe('fill');
    expect(rootAttribute(render({ variant: 'bare', size: 'medium' }), 'data-size')).toBe('medium');
  });

  it('colours by meaning from Kerf tokens and animates only through reduced-motion-aware transitions', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'glance-gauge.css'), 'utf8');
    expect(css).toContain('var(--kui-color-brand-fill-loud)');
    expect(css).toContain('var(--kui-color-success-fill-loud)');
    expect(css).toContain('var(--hs-standby-gauge-fill)');
    expect(css).toMatch(/@media \(prefers-reduced-motion: reduce\)[\s\S]*transition: none/);
    expect(css).not.toMatch(/\.kui-/);
    expect(css).not.toMatch(/@keyframes/);
  });
});
