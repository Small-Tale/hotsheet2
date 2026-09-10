import {readFileSync} from 'node:fs';
import {resolve} from 'node:path';

import {describe,expect,it} from 'vitest';

import {ProjectCloseDialog,type ProjectCloseResource,projectCloseResourceKey,projectCloseRunningSummary,selectedProjectCloseResource} from './project-close-dialog';

const resources:ProjectCloseResource[]=[{kind:'terminal',id:'term-one',name:'Tests',busy:true,cwd:'/work/demo',progress:68,preview:'Test Files 42 passed'},{kind:'terminal',id:'term-two',name:'Server',cwd:'/work/demo/server'},{kind:'ai-chat',id:'chat-one',name:'Codex chat',tool:'Codex',model:'gpt-6-astra',effort:'high',sessionId:'thread-1',preview:'Reviewing the close flow.'}];

describe('ProjectCloseDialog',()=>{
  it('stays absent until a project-close decision is requested',()=>{expect(String(ProjectCloseDialog({}))).toBe('')});

  it('renders running terminals and AI chats in one selectable list with a selected detail preview',()=>{
    const markup=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources,selectedKey:'ai-chat:chat-one'}}));
    expect(markup).toContain('data-component="project-close-dialog"');expect(markup).toContain('label="Close Demo?"');expect(markup).toContain('2 running terminals and 1 AI chat will stay active unless you close them first.');expect(markup).toContain('aria-label="Running terminals and AI chats"');expect(markup.match(/data-action="select-project-close-resource"/g)).toHaveLength(3);expect(markup).toContain('data-resource-key="ai-chat:chat-one" data-selected="true" aria-pressed="true"');expect(markup).toContain('Codex chat details');for(const value of ['Codex','gpt-6-astra','high','thread-1','Reviewing the close flow.'])expect(markup).toContain(value);expect(markup).toContain('data-lucide="message-square"');
  });

  it('falls back to the first item and exposes terminal-specific metadata',()=>{
    const markup=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources,selectedKey:'missing'}}));
    expect(markup).toContain('data-resource-key="terminal:term-one" data-selected="true" aria-pressed="true"');expect(markup).toContain('Tests details');expect(markup).toContain('/work/demo');expect(markup).toContain('68%');expect(markup).toContain('Test Files 42 passed');expect(markup).toContain('Terminal · Busy');expect(markup).toContain('data-lucide="square-terminal"');
  });

  it('provides distinct cancel, close-all, and close-project contracts with busy states',()=>{
    const ready=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources}}));for(const action of ['cancel-project-close','close-all-project-resources','confirm-close-project'])expect(ready).toContain(`data-action="${action}"`);expect(ready).toContain('Close All');expect(ready).toContain('Close Project');
    const busy=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources,operation:'closing-all',error:'Could not close Server.'}}));expect(busy).toContain('aria-busy="true"');expect(busy).toContain('Closing all…');expect(busy).toContain('role="alert"');expect(busy).toContain('Could not close Server.');expect(busy.match(/disabled/g)?.length).toBeGreaterThanOrEqual(6);
  });

  it('handles an empty running set and keeps Close All unavailable',()=>{
    const markup=String(ProjectCloseDialog({state:{projectId:'demo',projectName:'Demo',resources:[]}}));expect(markup).toContain('No terminals or AI chats are currently running for this project.');expect(markup).toContain('Nothing is running.');expect(markup).toMatch(/data-action="close-all-project-resources"[^>]*disabled/);expect(markup).toContain('Select Close Project to remove the project from this window.');
  });

  it('provides stable resource identity, selection, summary, and responsive two-column layout',()=>{
    expect(projectCloseResourceKey(resources[2])).toBe('ai-chat:chat-one');expect(selectedProjectCloseResource(resources,'terminal:term-two')).toBe(resources[1]);expect(selectedProjectCloseResource(resources,'unknown')).toBe(resources[0]);expect(projectCloseRunningSummary([resources[2]])).toBe('1 AI chat will stay active unless you close them first.');
    const css=readFileSync(resolve(import.meta.dirname,'project-close-dialog.css'),'utf8');expect(css).toMatch(/grid-template-columns:minmax\(14rem,18rem\) minmax\(0,1fr\)/);expect(css).toMatch(/@media \(max-width:42rem\)[\s\S]*grid-template-columns:1fr/);expect(css).toContain('cursor:pointer');expect(css).toContain('cursor:not-allowed');
  });
});
