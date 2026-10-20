import { describe, expect, it } from 'vitest';

import {
  closeTerminalVisibilityDemo,
  setAllTerminalVisibilityDemo,
  showTerminalVisibilityDemo,
  terminalVisibilityDemoState,
  TerminalVisibilityDialogDemo,
  toggleTerminalVisibilityDemoType,
} from './terminal-visibility-demo';

describe('TerminalVisibilityDialog demo', () => {
  it('persists type choices across reopening and scopes bulk changes to listed items', () => {
    const initial = terminalVisibilityDemoState.value;
    showTerminalVisibilityDemo();
    toggleTerminalVisibilityDemoType('shell');
    toggleTerminalVisibilityDemoType('ai');
    setAllTerminalVisibilityDemo(false);
    let markup = String(TerminalVisibilityDialogDemo());
    expect(markup).toContain('aria-label="Show Codex chat"');
    expect(markup).not.toContain('aria-label="Hide AI"');
    toggleTerminalVisibilityDemoType('chat');
    setAllTerminalVisibilityDemo(true);
    expect(terminalVisibilityDemoState.value.groups[1].hiddenKeys).toContain('demo:ai-chat:demo');
    closeTerminalVisibilityDemo();
    showTerminalVisibilityDemo();
    expect(terminalVisibilityDemoState.value.groups[1].hiddenTypes).toEqual(['shell', 'ai', 'chat']);
    toggleTerminalVisibilityDemoType('chat');
    setAllTerminalVisibilityDemo(true);
    markup = String(TerminalVisibilityDialogDemo());
    expect(markup).toContain('aria-label="Hide Codex chat"');
    closeTerminalVisibilityDemo();
    terminalVisibilityDemoState.value = initial;
  });
  it('starts behind a launcher and supports explicit open and close', () => {
    closeTerminalVisibilityDemo();
    const closed = String(TerminalVisibilityDialogDemo());
    expect(closed).toContain('Manage Workspace Visibility');
    expect(closed).toMatch(/data-terminal-visibility-dialog=""[^>]*data-controlled-open="false"/);
    showTerminalVisibilityDemo();
    expect(String(TerminalVisibilityDialogDemo())).toMatch(
      /data-terminal-visibility-dialog=""[^>]*data-controlled-open="true"/,
    );
    closeTerminalVisibilityDemo();
    expect(String(TerminalVisibilityDialogDemo())).not.toMatch(
      /data-terminal-visibility-dialog=""[^>]*data-controlled-open="true"/,
    );
  });
});
