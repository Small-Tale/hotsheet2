import { describe, expect, it } from 'vitest';

import { TerminalDashboard, type TerminalDashboardGroup } from './terminal-dashboard';

const groups:TerminalDashboardGroup[]=[{projectId:'one',projectName:'Project One',sessions:[{id:'term-1',projectId:'one',projectName:'Project One',title:'Codex',alive:true,busy:true,cwd:'/work/one',progress:42,scrollback:'Working\nRunning tests'}]}];

describe('TerminalDashboard',()=>{
  it('renders project groups, terminal state, actions, and across-scale controls',()=>{const markup=String(TerminalDashboard({groups,width:1200,height:700,fitAcross:4,fitHigh:2}));expect(markup).toContain('data-basis="across"');expect(markup).toContain('data-fit="4"');expect(markup).toContain('Project One');expect(markup).toContain('Running tests');expect(markup).toContain('42%');for(const action of ['magnify-terminal','open-terminal-project','dedicate-terminal','hide-dashboard-terminal','zoom-terminal-grid'])expect(markup).toContain(`data-action="${action}"`)});
  it('uses the short-container high scale and projects magnified and hidden state',()=>{const markup=String(TerminalDashboard({groups,width:900,height:600,fitAcross:7,fitHigh:3,magnifiedKey:'one:term-1',hiddenKeys:['one:term-1']}));expect(markup).toContain('data-basis="high"');expect(markup).toContain('data-fit="3"');expect(markup).toContain('Show hidden (1)');expect(markup).toContain('role="dialog"');expect(markup).toContain('disabled')});
});
