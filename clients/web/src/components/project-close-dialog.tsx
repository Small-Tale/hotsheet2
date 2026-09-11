import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './project-close-dialog.css';

import {CircleAlert,MessageSquare,SquareTerminal} from 'lucide';

import {LucideIcon} from './lucide-icon';
import {MarkdownPreview} from './markdown-preview';
import {MenuHeader} from './menu-header';
import {MenuItem} from './menu-item';

interface ProjectCloseResourceBase {
  id:string;
  name:string;
  busy?:boolean;
  preview?:string;
}

export interface ProjectCloseTerminal extends ProjectCloseResourceBase {
  kind:'terminal';
  cwd?:string;
  progress?:number;
}

export interface ProjectCloseAIChat extends ProjectCloseResourceBase {
  kind:'ai-chat';
  tool:string;
  model?:string;
  effort?:string;
  sessionId?:string;
}

export type ProjectCloseResource=ProjectCloseTerminal|ProjectCloseAIChat;

export interface ProjectCloseDialogState {
  projectId:string;
  projectName:string;
  resources:readonly ProjectCloseResource[];
  selectedKey?:string;
  operation?:'closing-all'|'closing-project';
  error?:string;
}

export function projectCloseResourceKey(resource:Pick<ProjectCloseResource,'kind'|'id'>):string{return `${resource.kind}:${resource.id}`}

export function selectedProjectCloseResource(resources:readonly ProjectCloseResource[],selectedKey?:string):ProjectCloseResource|undefined{return resources.find(resource=>projectCloseResourceKey(resource)===selectedKey)??resources[0]}

export function projectCloseRunningSummary(resources:readonly ProjectCloseResource[]):string{
  const terminals=resources.filter(resource=>resource.kind==='terminal').length,chats=resources.length-terminals,parts=[...(terminals?[`${terminals} running terminal${terminals===1?'':'s'}`]:[]),...(chats?[`${chats} AI chat${chats===1?'':'s'}`]:[])];
  return parts.length?`${parts.join(' and ')} will stay active unless you close them first.`:'No terminals or AI chats are currently running for this project.';
}

function ResourceDetail({resource,projectId}:{resource:ProjectCloseResource;projectId:string}){
  if(resource.kind==='terminal')return <section class="project-close-dialog__detail project-close-dialog__terminal" aria-label={`${resource.name} terminal preview`}>
    <div class="terminal-tile__viewport-frame"><div class="terminal-viewport terminal-viewport--scaled-preview" data-key={`project-close:${resource.id}`} data-morph-skip data-component="terminal-viewport" data-project-id={projectId} data-terminal-id={resource.id} data-display-mode="scaled-preview" data-grid-policy="dashboard-80x24" data-geometry-ready="false" aria-hidden="true"></div></div>
    <p class="project-close-dialog__preview-fallback">Connecting to the live terminal…</p>
  </section>;
  const preview=resource.preview?.trim();
  return <section class="project-close-dialog__detail project-close-dialog__chat" aria-label={`${resource.name} chat preview`}>
    <dl aria-label="AI chat session">
      <div><dt>Provider</dt><dd>{resource.tool}</dd></div>
      <div><dt>Model</dt><dd>{resource.model||'Provider default'}</dd></div>
      <div><dt>Effort</dt><dd>{resource.effort||'Provider default'}</dd></div>
    </dl>
    <section class="project-close-dialog__chat-activity" aria-label={`${resource.name} latest activity`}>{preview?<MarkdownPreview source={preview}/>:<p>No chat activity is available yet.</p>}</section>
  </section>;
}

export function ProjectCloseDialog({state}:{state?:ProjectCloseDialogState}){
  if(!state)return <></>;
  const selected=selectedProjectCloseResource(state.resources,state.selectedKey),busy=Boolean(state.operation),closingAll=state.operation==='closing-all',closingProject=state.operation==='closing-project';
  return <wa-dialog class="project-close-dialog" data-component="project-close-dialog" data-project-id={state.projectId} label={`Close ${state.projectName}?`} aria-describedby="project-close-dialog-summary" open>
    <div class="project-close-dialog__intro"><span><LucideIcon icon={CircleAlert} name="circle-alert"/></span><p id="project-close-dialog-summary">{projectCloseRunningSummary(state.resources)}</p></div>
    <div class="project-close-dialog__layout" aria-busy={String(busy)}>
      <aside aria-label="Running terminals and AI chats"><MenuHeader label="Running items"/>{state.resources.length?<nav>{state.resources.map(resource=>{const key=projectCloseResourceKey(resource),terminal=resource.kind==='terminal';return <MenuItem action="select-project-close-resource" itemId={key} selected={resource===selected} disabled={busy} icon={<LucideIcon icon={terminal?SquareTerminal:MessageSquare} name={terminal?'square-terminal':'message-square'}/>} label={resource.name} trailing={<small class="menu-item__count">{terminal?'Terminal':resource.tool} · {resource.busy?'Busy':'Running'}</small>}/>})}</nav>:<p class="project-close-dialog__empty">Nothing is running.</p>}</aside>
      {selected?<ResourceDetail resource={selected} projectId={state.projectId}/>:<section class="project-close-dialog__detail project-close-dialog__detail--empty"><p>Close this project tab?</p></section>}
    </div>
    <p class="project-close-dialog__consequences"><strong>Keep running</strong> closes only this tab. Terminals return when reopened; AI sessions remain on the server but do not reopen as tabs. <strong>Stop all</strong> ends every item, then closes the tab.</p>
    <p class="project-close-dialog__error" role="alert">{state.error}</p>
    <div slot="footer" class="project-close-dialog__actions"><wa-button type="button" size="small" appearance="outlined" data-action="cancel-project-close" disabled={busy}>Cancel</wa-button><wa-button type="button" size="small" appearance="outlined" data-action="confirm-close-project" data-project-id={state.projectId} disabled={busy}>{closingProject?'Closing…':'Keep Running'}</wa-button><wa-button type="button" size="small" variant="danger" data-action="close-all-project-resources" data-project-id={state.projectId} disabled={busy||state.resources.length===0}>{closingAll?'Stopping…':'Stop & Close'}</wa-button></div>
  </wa-dialog>;
}
