import { describe, expect, it } from 'vitest';

import { MainShell } from './main-shell';

describe('MainShell', () => {
  it('owns the configured AppShell rendering boundary', () => {
    const markup = String(MainShell({
      tabs: [],
      header: 'Demo project' as never,
      workspace: 'Queue' as never,
      inspectorVisible: false,
    }));
    expect(markup).toContain('data-component="app-shell"');
    expect(markup).toContain('Demo project');
    expect(markup).toContain('Queue');
  });
});
