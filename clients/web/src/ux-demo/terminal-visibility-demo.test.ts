import { describe, expect, it } from 'vitest';

import {
  closeTerminalVisibilityDemo,
  setAllTerminalVisibilityDemo,
  showTerminalVisibilityDemo,
  terminalVisibilityDemoState,
  terminalVisibilityDemoTypes,
  TerminalVisibilityDialogDemo,
} from './terminal-visibility-demo';

describe('TerminalVisibilityDialog demo', () => {
  it('projects type replacement and bulk chat visibility, resets, then accepts another edit', () => {
    showTerminalVisibilityDemo();
    terminalVisibilityDemoTypes.value = ['chat'];
    setAllTerminalVisibilityDemo(false);
    let markup = String(TerminalVisibilityDialogDemo());
    expect(markup).toContain('aria-label="Show Codex chat"');
    expect(markup).not.toContain('aria-label="Hide AI"');
    terminalVisibilityDemoTypes.value = [];
    setAllTerminalVisibilityDemo(true);
    expect(terminalVisibilityDemoState.value.groups[1].hiddenKeys).toContain('demo:ai-chat:demo');
    closeTerminalVisibilityDemo();
    showTerminalVisibilityDemo();
    expect(terminalVisibilityDemoTypes.value).toEqual(['shell', 'ai', 'chat']);
    terminalVisibilityDemoTypes.value = ['chat'];
    setAllTerminalVisibilityDemo(true);
    markup = String(TerminalVisibilityDialogDemo());
    expect(markup).toContain('aria-label="Hide Codex chat"');
    closeTerminalVisibilityDemo();
  });
  it('starts behind a launcher and supports explicit open and close', () => {
    closeTerminalVisibilityDemo();
    const closed = String(TerminalVisibilityDialogDemo());
    expect(closed).toContain('Manage Workspace Visibility');
    expect(closed).not.toMatch(/data-terminal-visibility-dialog[^>]*\sopen/);
    showTerminalVisibilityDemo();
    expect(String(TerminalVisibilityDialogDemo())).toMatch(/data-terminal-visibility-dialog[^>]*\sopen/);
    closeTerminalVisibilityDemo();
    expect(String(TerminalVisibilityDialogDemo())).not.toMatch(/data-terminal-visibility-dialog[^>]*\sopen/);
  });
});
