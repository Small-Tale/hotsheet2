import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { Toolbar } from './toolbar';
import { ToolbarControlGroup } from './toolbar-control-group';
import { ToolbarText } from './toolbar-text';

describe('toolbar primitives', () => {
  it('uses the specified dark-tone border', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'toolbar-control-group.css'), 'utf8');
    const dark = css.match(/\.toolbar-control-group\[data-tone="dark"\] \{([^}]+)\}/)?.[1] ?? '';
    expect(dark).toContain('--toolbar-control-border-color: var(--hs-toolbar-control-dark-border)');
    expect(readFileSync(resolve(import.meta.dirname, '../theme.css'), 'utf8')).toContain('--hs-toolbar-control-dark-border: #353536');
  });

  it('exposes orthogonal geometry, tone, and button appearance without changing structure', () => {
    const contained = String(ToolbarControlGroup({ children: 'control' as never }));
    const borderless = String(ToolbarControlGroup({ children: 'control' as never, appearance: 'borderless', single: true }));
    const darkPush = String(ToolbarControlGroup({ children: 'control' as never, tone: 'dark', buttonAppearance: 'push', single: true }));
    expect(contained).toContain('data-appearance="contained"');
    expect(contained).toContain('data-tone="default"');
    expect(contained).toContain('data-button-appearance="plain"');
    expect(borderless).toContain('data-appearance="borderless"');
    expect(borderless).toContain('data-single="true"');
    expect(darkPush).toContain('data-tone="dark"');
    expect(darkPush).toContain('data-button-appearance="push"');
  });

  it('projects every toolbar text size', () => {
    for (const size of ['large', 'default', 'small'] as const) {
      const markup = String(ToolbarText({ text: 'Identity', size }));
      expect(markup).toContain(`data-size="${size}"`);
      expect(markup).toContain('Identity');
    }
  });

  it('projects leading, center, trailing, and divider state', () => {
    const markup = String(Toolbar({ leading: 'left' as never, center: 'center' as never, trailing: 'right' as never, divider: false }));
    expect(markup.indexOf('>left<')).toBeLessThan(markup.indexOf('>center<'));
    expect(markup.indexOf('>center<')).toBeLessThan(markup.indexOf('>right<'));
    expect(markup).toContain('data-divider="false"');
    expect(markup).toContain('data-has-center="true"');
  });
});
