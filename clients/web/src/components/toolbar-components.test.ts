import { describe, expect, it } from 'vitest';
import { ToolbarControlGroup } from './toolbar-control-group';
import { ToolbarText } from './toolbar-text';
import { Toolbar } from './toolbar';

describe('toolbar primitives', () => {
  it('exposes contained and borderless control groups without changing their structure', () => {
    const contained = String(ToolbarControlGroup({ children: 'control' as never }));
    const borderless = String(ToolbarControlGroup({ children: 'control' as never, appearance: 'borderless', single: true }));
    expect(contained).toContain('data-appearance="contained"');
    expect(borderless).toContain('data-appearance="borderless"');
    expect(borderless).toContain('data-single="true"');
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
    expect(markup.indexOf('left')).toBeLessThan(markup.indexOf('center'));
    expect(markup.indexOf('center')).toBeLessThan(markup.indexOf('right'));
    expect(markup).toContain('data-divider="false"');
  });
});
