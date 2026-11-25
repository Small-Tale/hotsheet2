import { describe, expect, it } from 'vitest';

import {
  closeTerminalVisibilityDemo,
  setAllTerminalVisibilityDemo,
  setTerminalVisibilityDemoChatFeature,
  showTerminalVisibilityDemo,
  terminalVisibilityDemoState,
  TerminalVisibilityDialogDemo,
  toggleTerminalVisibilityDemoProject,
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
  it('drops the AI Chats segment while the demo chat feature is off and nothing is open (HS2-4TKASN)', () => {
    showTerminalVisibilityDemo();
    setTerminalVisibilityDemoChatFeature(false);
    let markup = String(TerminalVisibilityDialogDemo());
    expect(markup).not.toContain('data-visibility-type="chat"');
    expect(markup).toContain('data-action="toggle-terminal-visibility-demo-chat-feature"');
    setTerminalVisibilityDemoChatFeature(true);
    markup = String(TerminalVisibilityDialogDemo());
    expect(markup).toContain('data-visibility-type="chat"');
    closeTerminalVisibilityDemo();
  });
  it('toggles a project heading between show-all and hide-all (HS2-HC67SP)', () => {
    const initial = terminalVisibilityDemoState.value;
    showTerminalVisibilityDemo();
    // The seeded Focus group hides the development server, so the heading shows all.
    expect(String(TerminalVisibilityDialogDemo())).toContain('aria-label="Show All Demo project Items"');
    toggleTerminalVisibilityDemoProject('demo');
    expect(terminalVisibilityDemoState.value.groups[1].hiddenKeys).toEqual([]);
    expect(String(TerminalVisibilityDialogDemo())).toContain('aria-label="Hide All Demo project Items"');
    toggleTerminalVisibilityDemoProject('demo');
    expect(terminalVisibilityDemoState.value.groups[1].hiddenKeys).toEqual([
      'demo:ai',
      'demo:server',
      'demo:ai-chat:demo',
    ]);
    closeTerminalVisibilityDemo();
    terminalVisibilityDemoState.value = initial;
  });
});
