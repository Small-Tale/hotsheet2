import { describe,expect,it } from 'vitest';

import { TerminalDrawer } from './terminal-drawer';

const sessions=[{id:'one',projectId:'project',projectName:'Project',title:'Terminal 1',alive:true,busy:true,scrollback:'one'},{id:'two',projectId:'project',projectName:'Project',title:'Terminal 2',alive:true,busy:false,scrollback:'two'}];
const render=(selectedId:string='grid')=>String(TerminalDrawer({projectId:'project',projectName:'Project',sessions,width:900,height:320,fitAcross:2,fitHigh:2,selectedId}));
describe('TerminalDrawer',()=>{
  it('renders the compact grid tab rail and all drawer actions',()=>{const markup=render();expect(markup).toContain('data-mode="grid"');expect(markup).toContain('aria-label="Terminal grid"');expect(markup).toContain('Terminal 1');expect(markup).toContain('Terminal 2');for(const action of ['select-drawer-terminal','create-project-terminal','toggle-terminal-drawer'])expect(markup).toContain(`data-action="${action}"`)});
  it('selects one dedicated terminal without creating or copying its session',()=>{const markup=render('one');expect(markup).toContain('data-mode="dedicated"');expect(markup.match(/data-terminal-id="one"/g)?.length).toBeGreaterThanOrEqual(2);expect(markup.match(/data-component="terminal-viewport"/g)).toHaveLength(1)});
  it('offers recovery for project-scoped hidden terminals',()=>{expect(String(TerminalDrawer({projectId:'project',projectName:'Project',sessions,width:900,height:320,fitAcross:2,fitHigh:2,selectedId:'grid',hiddenKeys:['project:one']}))).toContain('Show 1 hidden project terminal')});
});
