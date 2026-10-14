import { describe, expect, it } from 'vitest';

import { ToolbarControlGroupDemo } from './toolbar-control-group-demo';

describe('ToolbarControlGroupDemo', () => {
  it('keeps all eight group variants in cataloged Toolbar zones', () => {
    const markup = String(ToolbarControlGroupDemo());
    expect(markup.match(/data-component="toolbar"/g)).toHaveLength(8);
    expect(markup.match(/data-component="toolbar-control-group"/g)).toHaveLength(8);
    for (const label of [
      'Segmented Choices',
      'Popup Menu',
      'Button Group',
      'Single Button',
      'Borderless Group',
      'Push Button, Resting',
      'Push Button, Pressed',
      'Dark Group',
    ])
      expect(markup).toContain(label);
  });
});
