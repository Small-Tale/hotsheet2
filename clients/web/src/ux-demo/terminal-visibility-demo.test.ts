import { describe,expect,it } from 'vitest';

import { closeTerminalVisibilityDemo,showTerminalVisibilityDemo,TerminalVisibilityDialogDemo } from './terminal-visibility-demo';

describe('TerminalVisibilityDialog demo',()=>{
  it('starts behind a launcher and supports explicit open and close',()=>{
    closeTerminalVisibilityDemo();
    const closed=String(TerminalVisibilityDialogDemo());
    expect(closed).toContain('Show / Hide Terminals');
    expect(closed).not.toMatch(/data-terminal-visibility-dialog[^>]*\sopen/);
    showTerminalVisibilityDemo();
    expect(String(TerminalVisibilityDialogDemo())).toMatch(/data-terminal-visibility-dialog[^>]*\sopen/);
    closeTerminalVisibilityDemo();
    expect(String(TerminalVisibilityDialogDemo())).not.toMatch(/data-terminal-visibility-dialog[^>]*\sopen/);
  });
});
