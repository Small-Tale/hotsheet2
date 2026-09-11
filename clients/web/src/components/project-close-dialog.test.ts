import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';

import {describe,expect,it} from 'vitest';

import {ProjectCloseDialog,type ProjectCloseResource,projectCloseResourceKey,projectCloseRunningSummary,selectedProjectCloseResource} from './project-close-dialog';

const resources:ProjectCloseResource[]=[{kind:'terminal',id:'term-one',name:'Tests',busy:true,cwd:'/work/demo',progress:68,preview:'Test Files 42 passed'},{kind:'terminal',id:'term-two',name:'Server',cwd:'/work/demo/server'},{kind:'ai-chat',id:'chat-one',name:'Codex chat',tool:'Codex',model:'gpt-6-astra',effort:'high',sessionId:'thread-1',preview:'Reviewing the close flow.'}];

describe('ProjectCloseDialog',()=>{
  it('stays absent until a project-close decision is requested',()=>{expect(String(ProjectCloseDialog({}))).toBe('')});

  it('uses shared menu primitives for running items and renders a selected chat without a redundant title',()=>{
    const markup=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources,selectedKey:'ai-chat:chat-one'}}));
    expect(markup).toContain('data-component="project-close-dialog"');expect(markup).toContain('label="Close Demo?"');expect(markup).toContain('2 running terminals and 1 AI chat will stay active unless you close them first.');expect(markup).toContain('aria-label="Running terminals and AI chats"');expect(markup).toContain('data-component="menu-header"');expect(markup.match(/data-component="menu-item"/g)).toHaveLength(3);expect(markup).toContain('data-item-id="ai-chat:chat-one"');expect(markup).toContain('aria-current="page"');expect(markup).toContain('aria-label="Codex chat chat preview"');for(const value of ['Codex','gpt-6-astra','high','Reviewing the close flow.'])expect(markup).toContain(value);expect(markup).not.toContain('thread-1');expect(markup).not.toContain('Codex chat details');expect(markup).toContain('data-lucide="message-square"');
  });

  it('falls back to the first item and embeds the live read-only terminal renderer',()=>{
    const markup=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources,selectedKey:'missing'}}));
    expect(markup).toContain('data-item-id="terminal:term-one"');expect(markup).toContain('aria-current="page"');expect(markup).toContain('aria-label="Tests terminal preview"');expect(markup).toContain('data-component="terminal-viewport"');expect(markup).toContain('data-project-id="demo"');expect(markup).toContain('data-terminal-id="term-one"');expect(markup).toContain('data-display-mode="scaled-preview"');expect(markup).not.toContain('/work/demo');expect(markup).not.toContain('68%');expect(markup).not.toContain('Test Files 42 passed');expect(markup).toContain('data-lucide="square-terminal"');
  });

  it('names the keep-running and stop-all consequences explicitly with busy states',()=>{
    const ready=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources}}));for(const action of ['cancel-project-close','close-all-project-resources','confirm-close-project'])expect(ready).toContain(`data-action="${action}"`);
    expect(ready).toContain('Stop &amp; Close');expect(ready).toContain('Keep Running');expect(ready).toContain('Terminals return when reopened');expect(ready).toContain('AI sessions remain on the server but do not reopen as tabs');
    const busy=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources,operation:'closing-all',error:'Could not close Server.'}}));expect(busy).toContain('aria-busy="true"');expect(busy).toContain('Stopping…');expect(busy).toContain('role="alert"');expect(busy).toContain('Could not close Server.');expect(busy.match(/disabled/g)?.length).toBeGreaterThanOrEqual(6);
  });

  it('handles an empty running set and keeps Close All unavailable',()=>{
    const markup=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources:[]}}));expect(markup).toContain('No terminals or AI chats are currently running for this project.');expect(markup).toContain('Nothing is running.');expect(markup).toMatch(/data-action="close-all-project-resources"[^>]*disabled/);expect(markup).toContain('Close this project tab?');
  });

  it('provides stable resource identity, selection, summary, and responsive two-column layout',()=>{
    expect(projectCloseResourceKey(resources[2])).toBe('ai-chat:chat-one');expect(selectedProjectCloseResource(resources,'terminal:term-two')).toBe(resources[1]);expect(selectedProjectCloseResource(resources,'unknown')).toBe(resources[0]);expect(projectCloseRunningSummary([resources[2]])).toBe('1 AI chat will stay active unless you close them first.');
    const css=readFileSync(resolve(import.meta.dirname,'project-close-dialog.css'),'utf8');expect(css).toMatch(/grid-template-columns:minmax\(14rem,18rem\) minmax\(0,1fr\)/);expect(css).toMatch(/@media \(max-width:42rem\)[\s\S]*grid-template-columns:1fr/);expect(css).toContain('background:var(--wa-color-surface-default)');expect(css).toContain('.project-close-dialog__terminal .terminal-viewport--scaled-preview');
  });
});
