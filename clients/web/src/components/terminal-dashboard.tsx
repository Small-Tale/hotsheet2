import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import './terminal-dashboard.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Ellipsis, ExternalLink, Eye, EyeOff, MessageSquare, Minus, Plus } from 'lucide';

import { TERMINAL_TILE_HORIZONTAL_CHROME, TERMINAL_TILE_VERTICAL_CHROME, terminalDrawerGridLayout, terminalGridLayout, terminalPreviewText } from '../terminal-grid-layout';
import { terminalPhysicalScale } from '../terminal-viewport';
import type { TerminalVisibilityGroup } from '../terminal-visibility';
import { Select } from './select';

export interface TerminalDashboardSession {
  id: string;
  projectId: string;
  projectName: string;
  title?: string;
  alive: boolean;
  busy: boolean;
  cwd?: string;
  progress?: number;
  scrollback: string;
}

export interface TerminalDashboardGroup {
  projectId: string;
  projectName: string;
  sessions: TerminalDashboardSession[];
  chats?: WorkspaceGridChat[];
  itemOrder?: string[];
}

export interface WorkspaceGridChat {
  id: string;
  projectId: string;
  projectName: string;
  name: string;
  tool: string;
  busy?: boolean;
  summary?: string;
}

export interface TerminalDashboardProps {
  groups: TerminalDashboardGroup[];
  width: number;
  height: number;
  fitAcross: number;
  fitHigh: number;
  grouping?: 'project' | 'flow';
  layoutMode?:'responsive'|'drawer';
  visibilityGroups?:readonly TerminalVisibilityGroup[];
  activeVisibilityGroupId?:string;
  visibilityScope?:string;
  magnifiedKey?: string;
  hiddenKeys?: readonly string[];
  loading?: boolean;
  message?: string;
  contextMenu?: {key:string;x:number;y:number};
}

const keyFor = (session: TerminalDashboardSession) => `${session.projectId}:${session.id}`;
const chatKeyFor = (chat: WorkspaceGridChat) => `${chat.projectId}:${chat.id}`;
export const WORKSPACE_CHAT_PREVIEW_NATURAL_WIDTH=300;
export const WORKSPACE_CHAT_PREVIEW_NATURAL_HEIGHT=180;
export function TerminalVisibilityControls({hiddenCount=0,groups=[],activeId='default',scope='dashboard'}:{hiddenCount?:number;groups?:readonly TerminalVisibilityGroup[];activeId?:string;scope?:string}){
  const choices=groups.map(group=>({value:group.id,label:group.name}));
  return <div class="terminal-dashboard-controls__visibility-group" data-visibility-scope={scope}><ToolbarControlGroup single><button type="button" class="terminal-dashboard-controls__visibility" data-action="open-terminal-visibility" aria-label="Manage terminal visibility" title="Show / Hide Terminals"><LucideIcon icon={Eye} name="eye" />{hiddenCount>0&&<span class="terminal-dashboard-controls__count" aria-hidden="true">{hiddenCount}</span>}</button></ToolbarControlGroup><ToolbarControlGroup single><Select className="terminal-dashboard-controls__visibility-select" name="terminal-visibility-group" ariaLabel="Terminal visibility group" value={activeId} choices={choices} renderSelected={choice=><span>{choice.label}</span>} /></ToolbarControlGroup></div>;
}

export function TerminalDashboardControls({ hiddenCount = 0,visibilityGroups=[],activeVisibilityGroupId='default',visibilityScope='dashboard' }: {hiddenCount?:number;visibilityGroups?:readonly TerminalVisibilityGroup[];activeVisibilityGroupId?:string;visibilityScope?:string}) {
  return <div class="terminal-dashboard-controls" data-component="terminal-dashboard-controls">
    <TerminalVisibilityControls hiddenCount={hiddenCount} groups={visibilityGroups} activeId={activeVisibilityGroupId} scope={visibilityScope}/>
  </div>;
}

export function FixedAspectTerminalCard({ session, mode = 'preview' }: {session:TerminalDashboardSession;mode?:'preview'|'magnified'}) {
  const key = keyFor(session);
  const dashboardPreview=mode==='preview',magnified=mode==='magnified';
  const preview = terminalPreviewText(session.scrollback) || 'Terminal is ready.';
  const viewport=<div class={`terminal-viewport${dashboardPreview?' terminal-viewport--scaled-preview':''}`} data-key={`${dashboardPreview?'preview':'viewport'}:${key}`} data-morph-skip data-component="terminal-viewport" data-project-id={session.projectId} data-terminal-id={session.id} data-display-mode={dashboardPreview?'scaled-preview':'interactive'} data-mount-policy={dashboardPreview?'visible-progressive':'immediate'} data-grid-policy="dashboard-80x24" data-geometry-ready="false" aria-hidden={dashboardPreview?'true':undefined} aria-label={dashboardPreview?undefined:`${session.title??session.id} interactive terminal`}></div>;
  return <article class="terminal-tile" data-key={key} data-component="terminal-tile" data-fixed-aspect-terminal-card={mode} data-terminal-key={key} data-busy={String(session.busy)} data-alive={String(session.alive)} data-magnified={String(magnified)} data-preview-only={String(dashboardPreview)} data-action={dashboardPreview?'preview-terminal':undefined} tabindex={dashboardPreview?'0':undefined} aria-label={dashboardPreview?`Preview ${session.title??session.id}`:undefined}>
    <div class="terminal-tile__preview"><pre>{preview}</pre><div class="terminal-tile__viewport-frame">{viewport}</div></div>
    <footer class="terminal-tile__footer">
      <span class="terminal-tile__state" aria-label={session.busy ? 'Busy' : session.alive ? 'Idle' : 'Exited'} title={session.busy ? 'Busy' : session.alive ? 'Idle' : 'Exited'}></span>
      <button type="button" class="terminal-tile__identity" data-action="open-terminal-project" data-item-id={key} aria-label={`Open ${session.title??session.id} in ${session.projectName}`}><strong>{session.projectName}<span aria-hidden="true"> › </span>{session.title ?? session.id}</strong></button>
      {session.progress !== undefined && <span class="terminal-tile__progress">{session.progress}%</span>}
      <button type="button" class="terminal-tile__menu" data-action="open-terminal-context-menu" data-item-id={key} aria-label={`More actions for ${session.title??session.id}`} title="More actions"><LucideIcon icon={Ellipsis} name="ellipsis"/></button>
      {magnified&&<button type="button" class="terminal-tile__open" data-action="open-terminal-project" data-terminal-key={key} aria-label={`Open ${session.title??session.id} in project terminal drawer`} title="Open in project terminal drawer"><LucideIcon icon={ExternalLink} name="external-link"/></button>}
    </footer>
  </article>;
}

export function TerminalSession({ session }: {session:TerminalDashboardSession}) {
  return <section class="terminal-session" data-key={keyFor(session)} data-component="terminal-session" data-terminal-key={keyFor(session)} aria-label={`${session.title??session.id} terminal`}>
    <div class="terminal-viewport terminal-viewport--dedicated" data-key={`dedicated-viewport:${keyFor(session)}`} data-morph-skip data-component="terminal-viewport" data-display-mode="interactive" data-project-id={session.projectId} data-terminal-id={session.id} aria-label={`${session.title??session.id} interactive terminal`}></div>
  </section>;
}

export function WorkspaceGridChatCard({chat,previewScale=1}:{chat:WorkspaceGridChat;previewScale?:number}){
  const key=chatKeyFor(chat),state=chat.busy?'Working':'Ready',scale=Math.max(0,previewScale);
  return <article class="terminal-tile workspace-chat-tile" data-key={key} data-component="workspace-chat-tile" data-chat-key={key} data-project-id={chat.projectId} data-chat-id={chat.id} data-busy={String(Boolean(chat.busy))} data-preview-only="true" data-action="open-grid-ai-chat" data-item-id={key} tabindex="0" aria-label={`Open ${chat.name} in ${chat.projectName}`}>
    <div class="terminal-tile__preview workspace-chat-tile__preview"><div class="workspace-chat-tile__preview-surface" data-preview-scale={String(scale)} style={`--workspace-chat-preview-scale:${scale};--workspace-chat-preview-natural-width:${WORKSPACE_CHAT_PREVIEW_NATURAL_WIDTH}px;--workspace-chat-preview-natural-height:${WORKSPACE_CHAT_PREVIEW_NATURAL_HEIGHT}px`}><LucideIcon icon={MessageSquare} name="message-square"/><div><strong>{chat.tool} AI chat</strong><p>{chat.summary||`Open ${chat.name} to continue the conversation.`}</p></div></div></div>
    <footer class="terminal-tile__footer"><span class="terminal-tile__state" aria-label={state} title={state}></span><button type="button" class="terminal-tile__identity" data-action="open-grid-ai-chat" data-item-id={key} data-project-id={chat.projectId} data-chat-id={chat.id} aria-label={`Open ${chat.name} in ${chat.projectName}`}><strong>{chat.projectName}<span aria-hidden="true"> › </span>{chat.name}</strong></button></footer>
  </article>;
}

type GridItem={kind:'terminal';session:TerminalDashboardSession}|{kind:'chat';chat:WorkspaceGridChat};

function Grid({ sessions,chats=[],itemOrder=[], layout }: {sessions:TerminalDashboardSession[];chats?:WorkspaceGridChat[];itemOrder?:string[];layout:ReturnType<typeof terminalGridLayout>}) {
  const style = `--terminal-tile-width:${layout.tileWidth}px;--terminal-tile-height:${layout.tileHeight}px;--terminal-grid-fit:${layout.fit}`,chatPreviewScale=terminalPhysicalScale(WORKSPACE_CHAT_PREVIEW_NATURAL_WIDTH,WORKSPACE_CHAT_PREVIEW_NATURAL_HEIGHT,Math.max(1,layout.tileWidth-TERMINAL_TILE_HORIZONTAL_CHROME),Math.max(1,layout.tileHeight-TERMINAL_TILE_VERTICAL_CHROME));
  const rank=new Map(itemOrder.map((id,index)=>[id,index])),items:GridItem[]=[...sessions.map(session=>({kind:'terminal' as const,session})),...chats.map(chat=>({kind:'chat' as const,chat}))];
  if(itemOrder.length)items.sort((left,right)=>(rank.get(left.kind==='terminal'?left.session.id:left.chat.id)??Number.MAX_SAFE_INTEGER)-(rank.get(right.kind==='terminal'?right.session.id:right.chat.id)??Number.MAX_SAFE_INTEGER));
  return <div class="terminal-grid" data-component="terminal-grid" data-basis={layout.basis} data-fit={String(layout.fit)} style={style}>{items.map(item=>item.kind==='terminal'?<FixedAspectTerminalCard session={item.session}/>:<WorkspaceGridChatCard chat={item.chat} previewScale={chatPreviewScale}/>)}</div>;
}

export function TerminalDashboard({ groups, width, height, fitAcross, fitHigh, grouping = 'flow',layoutMode='responsive', magnifiedKey, hiddenKeys = [], loading = false, message = '',contextMenu }: TerminalDashboardProps) {
  const hidden = new Set(hiddenKeys);
  const visibleGroups = groups.map(group => ({ ...group, sessions: group.sessions.filter(session => !hidden.has(keyFor(session))),chats:(group.chats??[]).filter(chat=>!hidden.has(chatKeyFor(chat))) })).filter(group => group.sessions.length+group.chats.length > 0);
  const sessions = visibleGroups.flatMap(group => group.sessions);
  const chats=visibleGroups.flatMap(group=>group.chats);
  const layout = layoutMode==='drawer'?terminalDrawerGridLayout(width,height,fitHigh):terminalGridLayout(width, height, fitAcross, fitHigh);
  const magnified = groups.flatMap(group => group.sessions).find(session => keyFor(session) === magnifiedKey);
  return <section class="terminal-dashboard" data-component="terminal-dashboard" data-layout-mode={layoutMode} data-basis={layout.basis} data-fit={String(layout.fit)} aria-label="Workspace grid">
    <div class="terminal-dashboard__content" data-terminal-grid-measure="true">
      {loading ? <div class="terminal-dashboard__empty" role="status">Loading workspace items…</div> : message ? <div class="terminal-dashboard__empty" role="status">{message}</div> : sessions.length+chats.length === 0 ? <div class="terminal-dashboard__empty"><strong>Nothing open yet</strong><span>Open a project terminal or AI chat to add it to this grid.</span></div> : grouping === 'flow' ? <Grid sessions={sessions} chats={chats} itemOrder={visibleGroups.length===1?visibleGroups[0].itemOrder:undefined} layout={layout} /> : visibleGroups.map(group => <section class="terminal-dashboard__project" data-key={group.projectId} data-project-id={group.projectId}><h2>{group.projectName}<span>{group.sessions.length+group.chats.length}</span></h2><Grid sessions={group.sessions} chats={group.chats} itemOrder={group.itemOrder} layout={layout} /></section>)}
    </div>
    <div class="terminal-dashboard__zoom" role="group" aria-label="Workspace tile zoom">
      <button type="button" data-action="zoom-terminal-grid" data-zoom-direction="out" disabled={layout.fit >= layout.max} aria-label={`Zoom out, fit more items ${layout.basis}`} title="Zoom out"><LucideIcon icon={Minus} name="minus" /></button>
      <button type="button" data-action="zoom-terminal-grid" data-zoom-direction="in" disabled={layout.fit <= 1} aria-label={`Zoom in, fit fewer items ${layout.basis}`} title="Zoom in"><LucideIcon icon={Plus} name="plus" /></button>
    </div>
    {magnified && <div class="terminal-dashboard__magnified" role="dialog" aria-modal="true" aria-label={`Magnified ${magnified.title ?? magnified.id}`} data-action="dismiss-magnified-terminal"><FixedAspectTerminalCard session={magnified} mode="magnified" /></div>}
    {contextMenu&&<div class="terminal-dashboard__context-menu" data-component="terminal-context-menu" role="menu" style={`left:${contextMenu.x}px;top:${contextMenu.y}px`} data-terminal-key={contextMenu.key}><wa-dropdown-item data-action="open-terminal-project" data-item-id={contextMenu.key}><span slot="icon"><LucideIcon icon={ExternalLink} name="external-link"/></span>Open</wa-dropdown-item><wa-dropdown-item data-action="hide-dashboard-terminal" data-item-id={contextMenu.key}><span slot="icon"><LucideIcon icon={EyeOff} name="eye-off"/></span>Hide Terminal</wa-dropdown-item></div>}
  </section>;
}
