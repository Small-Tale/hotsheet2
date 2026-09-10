import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './project-close-dialog.css';

import {CircleAlert,MessageSquare,SquareTerminal} from 'lucide';

import {LucideIcon} from './lucide-icon';

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

function ResourceDetail({resource}:{resource:ProjectCloseResource}){
  const terminal=resource.kind==='terminal',preview=resource.preview?.trim();
  return <section class="project-close-dialog__detail" aria-label={`${resource.name} details`}>
    <header><span class="project-close-dialog__detail-icon"><LucideIcon icon={terminal?SquareTerminal:MessageSquare} name={terminal?'square-terminal':'message-square'}/></span><div><p>{terminal?'Terminal':'AI chat'} · {resource.busy?'Busy':'Running'}</p><h3>{resource.name}</h3></div></header>
    <dl aria-label={`${resource.name} metadata`}>
      {terminal?<><div><dt>Working directory</dt><dd><code>{resource.cwd||'Not reported'}</code></dd></div><div><dt>Progress</dt><dd>{resource.progress===undefined?'Not reported':`${resource.progress}%`}</dd></div></>:<><div><dt>Provider</dt><dd>{resource.tool}</dd></div><div><dt>Model</dt><dd>{resource.model||'Provider default'}</dd></div><div><dt>Effort</dt><dd>{resource.effort||'Provider default'}</dd></div><div><dt>Session</dt><dd><code>{resource.sessionId||'Not started'}</code></dd></div></>}
    </dl>
    <section class="project-close-dialog__preview"><h4>{terminal?'Recent output':'Latest activity'}</h4><pre aria-label={`${resource.name} preview`}>{preview||`No recent ${terminal?'terminal output':'chat activity'} available.`}</pre></section>
  </section>;
}

export function ProjectCloseDialog({state}:{state?:ProjectCloseDialogState}){
  if(!state)return <></>;
  const selected=selectedProjectCloseResource(state.resources,state.selectedKey),busy=Boolean(state.operation),closingAll=state.operation==='closing-all',closingProject=state.operation==='closing-project';
  return <wa-dialog class="project-close-dialog" data-component="project-close-dialog" data-project-id={state.projectId} label={`Close ${state.projectName}?`} aria-describedby="project-close-dialog-summary" open>
    <div class="project-close-dialog__intro"><span><LucideIcon icon={CircleAlert} name="circle-alert"/></span><p id="project-close-dialog-summary">{projectCloseRunningSummary(state.resources)}</p></div>
    <div class="project-close-dialog__layout" aria-busy={String(busy)}>
      <aside aria-label="Running terminals and AI chats"><header><strong>Running items</strong><span>{state.resources.length}</span></header>{state.resources.length?<ul>{state.resources.map(resource=>{const key=projectCloseResourceKey(resource),terminal=resource.kind==='terminal',active=resource===selected;return <li data-key={key}><button type="button" data-action="select-project-close-resource" data-resource-key={key} data-selected={String(active)} aria-pressed={String(active)} disabled={busy}><LucideIcon icon={terminal?SquareTerminal:MessageSquare} name={terminal?'square-terminal':'message-square'}/><span><strong>{resource.name}</strong><small>{terminal?'Terminal':resource.tool} · {resource.busy?'Busy':'Running'}</small></span></button></li>})}</ul>:<p class="project-close-dialog__empty">Nothing is running.</p>}</aside>
      {selected?<ResourceDetail resource={selected}/>:<section class="project-close-dialog__detail project-close-dialog__detail--empty"><p>Select Close Project to remove the project from this window.</p></section>}
    </div>
    <p class="project-close-dialog__error" role="alert">{state.error}</p>
    <div slot="footer" class="project-close-dialog__actions"><wa-button type="button" appearance="outlined" data-action="cancel-project-close" disabled={busy}>Cancel</wa-button><wa-button type="button" appearance="outlined" variant="danger" data-action="close-all-project-resources" data-project-id={state.projectId} disabled={busy||state.resources.length===0}>{closingAll?'Closing all…':'Close All'}</wa-button><wa-button type="button" variant="danger" data-action="confirm-close-project" data-project-id={state.projectId} disabled={busy}>{closingProject?'Closing project…':'Close Project'}</wa-button></div>
  </wa-dialog>;
}
