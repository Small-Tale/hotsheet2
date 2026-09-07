import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import './terminal-dashboard.css';

import { ExternalLink, Eye, EyeOff, Minus, Plus } from 'lucide';

import { terminalDrawerGridLayout, terminalGridLayout, terminalPreviewText } from '../terminal-grid-layout';
import type { TerminalVisibilityGroup } from '../terminal-visibility';
import { LucideIcon } from './lucide-icon';
import { Select } from './select';
import { ToolbarControlGroup } from './toolbar-control-group';

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
export function TerminalVisibilityControls({hiddenCount=0,groups=[],activeId='default',scope='dashboard'}:{hiddenCount?:number;groups?:readonly TerminalVisibilityGroup[];activeId?:string;scope?:string}){
  const choices=groups.map(group=>({value:group.id,label:group.name}));
  return <div class="terminal-dashboard-controls__visibility-group" data-visibility-scope={scope}><ToolbarControlGroup single><button type="button" class="terminal-dashboard-controls__visibility" data-action="open-terminal-visibility" aria-label="Manage terminal visibility" title="Show / Hide Terminals"><LucideIcon icon={Eye} name="eye" />{hiddenCount>0&&<span class="terminal-dashboard-controls__count" aria-hidden="true">{hiddenCount}</span>}</button></ToolbarControlGroup><ToolbarControlGroup single><Select className="terminal-dashboard-controls__visibility-select" name="terminal-visibility-group" ariaLabel="Terminal visibility group" value={activeId} choices={choices} fitMenu renderSelected={choice=><span>{choice.label}</span>} /></ToolbarControlGroup></div>;
}

export function TerminalDashboardControls({ hiddenCount = 0,visibilityGroups=[],activeVisibilityGroupId='default',visibilityScope='dashboard' }: {hiddenCount?:number;visibilityGroups?:readonly TerminalVisibilityGroup[];activeVisibilityGroupId?:string;visibilityScope?:string}) {
  return <div class="terminal-dashboard-controls" data-component="terminal-dashboard-controls">
    <TerminalVisibilityControls hiddenCount={hiddenCount} groups={visibilityGroups} activeId={activeVisibilityGroupId} scope={visibilityScope}/>
  </div>;
}

function TerminalTile({ session, magnified = false, dashboardPreview = false }: {session:TerminalDashboardSession;magnified?:boolean;dashboardPreview?:boolean}) {
  const key = keyFor(session);
  const preview = terminalPreviewText(session.scrollback) || 'Terminal is ready.';
  const viewport=<div class={`terminal-viewport${dashboardPreview?' terminal-viewport--scaled-preview':''}`} data-key={`${dashboardPreview?'preview':'viewport'}:${key}`} data-morph-skip data-component="terminal-viewport" data-project-id={session.projectId} data-terminal-id={session.id} data-display-mode={dashboardPreview?'scaled-preview':'interactive'} data-grid-policy="dashboard-80x24" aria-hidden={dashboardPreview?'true':undefined} aria-label={dashboardPreview?undefined:`${session.title??session.id} interactive terminal`}></div>;
  return <article class="terminal-tile" data-key={key} data-component="terminal-tile" data-terminal-key={key} data-busy={String(session.busy)} data-alive={String(session.alive)} data-magnified={String(magnified)} data-preview-only={String(dashboardPreview)} data-action={dashboardPreview?'preview-terminal':undefined} tabindex={dashboardPreview?'0':undefined} aria-label={dashboardPreview?`Preview ${session.title??session.id}`:undefined}>
    <div class="terminal-tile__preview"><pre>{preview}</pre>{dashboardPreview?<div class="terminal-tile__viewport-frame">{viewport}</div>:viewport}</div>
    <footer class="terminal-tile__footer">
      <span class="terminal-tile__state" aria-label={session.busy ? 'Busy' : session.alive ? 'Idle' : 'Exited'} title={session.busy ? 'Busy' : session.alive ? 'Idle' : 'Exited'}></span>
      <button type="button" class="terminal-tile__identity" data-action="open-terminal-project" data-item-id={key} aria-label={`Open ${session.title??session.id} in ${session.projectName}`}><strong>{session.projectName}<span aria-hidden="true"> › </span>{session.title ?? session.id}</strong></button>
      {session.progress !== undefined && <span class="terminal-tile__progress">{session.progress}%</span>}
      {magnified&&<button type="button" class="terminal-tile__open" data-action="open-terminal-project" data-terminal-key={key} aria-label={`Open ${session.title??session.id} in project terminal drawer`} title="Open in project terminal drawer"><LucideIcon icon={ExternalLink} name="external-link"/></button>}
    </footer>
  </article>;
}

export function TerminalSession({ session }: {session:TerminalDashboardSession}) {
  return <section class="terminal-session" data-key={keyFor(session)} data-component="terminal-session" data-terminal-key={keyFor(session)} aria-label={`${session.title??session.id} terminal`}>
    <div class="terminal-viewport terminal-viewport--dedicated" data-key={`dedicated-viewport:${keyFor(session)}`} data-morph-skip data-component="terminal-viewport" data-display-mode="interactive" data-project-id={session.projectId} data-terminal-id={session.id} aria-label={`${session.title??session.id} interactive terminal`}></div>
  </section>;
}

function Grid({ sessions, layout }: {sessions:TerminalDashboardSession[];layout:ReturnType<typeof terminalGridLayout>}) {
  const style = `--terminal-tile-width:${layout.tileWidth}px;--terminal-tile-height:${layout.tileHeight}px;--terminal-grid-fit:${layout.fit}`;
  return <div class="terminal-grid" data-component="terminal-grid" data-basis={layout.basis} data-fit={String(layout.fit)} style={style}>{sessions.map(session => <TerminalTile session={session} dashboardPreview/>)}</div>;
}

export function TerminalDashboard({ groups, width, height, fitAcross, fitHigh, grouping = 'flow',layoutMode='responsive', magnifiedKey, hiddenKeys = [], loading = false, message = '',contextMenu }: TerminalDashboardProps) {
  const hidden = new Set(hiddenKeys);
  const visibleGroups = groups.map(group => ({ ...group, sessions: group.sessions.filter(session => !hidden.has(keyFor(session))) })).filter(group => group.sessions.length > 0);
  const sessions = visibleGroups.flatMap(group => group.sessions);
  const layout = layoutMode==='drawer'?terminalDrawerGridLayout(width,height,fitHigh):terminalGridLayout(width, height, fitAcross, fitHigh);
  const magnified = groups.flatMap(group => group.sessions).find(session => keyFor(session) === magnifiedKey);
  return <section class="terminal-dashboard" data-component="terminal-dashboard" data-layout-mode={layoutMode} data-basis={layout.basis} data-fit={String(layout.fit)} aria-label="Terminal dashboard">
    <div class="terminal-dashboard__content" data-terminal-grid-measure="true">
      {loading ? <div class="terminal-dashboard__empty" role="status">Loading terminals…</div> : message ? <div class="terminal-dashboard__empty" role="status">{message}</div> : sessions.length === 0 ? <div class="terminal-dashboard__empty"><strong>No active terminals</strong><span>Open a project terminal to add it to this dashboard.</span></div> : grouping === 'flow' ? <Grid sessions={sessions} layout={layout} /> : visibleGroups.map(group => <section class="terminal-dashboard__project" data-key={group.projectId} data-project-id={group.projectId}><h2>{group.projectName}<span>{group.sessions.length}</span></h2><Grid sessions={group.sessions} layout={layout} /></section>)}
    </div>
    <div class="terminal-dashboard__zoom" role="group" aria-label="Terminal tile zoom">
      <button type="button" data-action="zoom-terminal-grid" data-zoom-direction="out" disabled={layout.fit >= layout.max} aria-label={`Zoom out, fit more terminals ${layout.basis}`} title="Zoom out"><LucideIcon icon={Minus} name="minus" /></button>
      <button type="button" data-action="zoom-terminal-grid" data-zoom-direction="in" disabled={layout.fit <= 1} aria-label={`Zoom in, fit fewer terminals ${layout.basis}`} title="Zoom in"><LucideIcon icon={Plus} name="plus" /></button>
    </div>
    {magnified && <div class="terminal-dashboard__magnified" role="dialog" aria-modal="true" aria-label={`Magnified ${magnified.title ?? magnified.id}`} data-action="dismiss-magnified-terminal"><TerminalTile session={magnified} magnified /></div>}
    {contextMenu&&<div class="terminal-dashboard__context-menu" data-component="terminal-context-menu" role="menu" style={`left:${contextMenu.x}px;top:${contextMenu.y}px`} data-terminal-key={contextMenu.key}><wa-dropdown-item data-action="open-terminal-project" data-item-id={contextMenu.key}><span slot="icon"><LucideIcon icon={ExternalLink} name="external-link"/></span>Open</wa-dropdown-item><wa-dropdown-item data-action="hide-dashboard-terminal" data-item-id={contextMenu.key}><span slot="icon"><LucideIcon icon={EyeOff} name="eye-off"/></span>Hide Terminal</wa-dropdown-item></div>}
  </section>;
}
