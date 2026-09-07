import { describe,expect,it } from 'vitest';

import { initialTerminalVisibilityState } from '../terminal-visibility';
import { TerminalVisibilityDialog } from './terminal-visibility-dialog';

const groups=[{projectId:'project',projectName:'Project',sessions:[{id:'one',projectId:'project',projectName:'Project',title:'AI',alive:true,busy:false,scrollback:''}]}];
describe('TerminalVisibilityDialog',()=>{
  it('uses tabs and shared menu rows for the active grouping',()=>{const markup=String(TerminalVisibilityDialog({open:true,state:initialTerminalVisibilityState(),scope:'dashboard',groups}));expect(markup).toContain('role="tablist"');expect(markup).toContain('aria-selected="true"');expect(markup).toContain('data-action="add-terminal-visibility-group"');expect(markup).toContain('data-action="toggle-terminal-visibility"');expect(markup).toContain('aria-label="Hide AI"');expect(markup).toContain('>Visible</span>')});
  it('limits a project scope to that project and protects Default management',()=>{const markup=String(TerminalVisibilityDialog({open:true,state:initialTerminalVisibilityState(),scope:'project:other',groups:[...groups,{...groups[0],projectId:'other',projectName:'Other',sessions:[{...groups[0].sessions[0],projectId:'other',projectName:'Other'}]}]}));expect(markup).not.toContain('<h2>Project</h2>');expect(markup).toContain('<h2>Other</h2>');expect(markup).toMatch(/name="terminal-visibility-group-name"[^>]*disabled/);expect(markup).toMatch(/data-action="remove-terminal-visibility-group"[^>]*disabled/)})
});
