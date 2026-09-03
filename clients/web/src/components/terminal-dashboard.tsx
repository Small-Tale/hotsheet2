import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dropdown/dropdown.js';
import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import './terminal-dashboard.css';

import { ExternalLink, Eye, EyeOff, LayoutGrid, Maximize2, Minus, Plus, Rows3, X } from 'lucide';

import { terminalGridLayout, terminalPreviewText } from '../terminal-grid-layout';
import { LucideIcon } from './lucide-icon';
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
  magnifiedKey?: string;
  hiddenKeys?: readonly string[];
  loading?: boolean;
  message?: string;
}

const keyFor = (session: TerminalDashboardSession) => `${session.projectId}:${session.id}`;

export function TerminalDashboardControls({ grouping = 'project', hiddenCount = 0 }: {grouping?:'project'|'flow';hiddenCount?:number}) {
  return <div class="terminal-dashboard-controls" data-component="terminal-dashboard-controls">
    <ToolbarControlGroup appearance="borderless" single>
      <wa-button appearance="plain" data-action="show-hidden-terminals" disabled={hiddenCount===0} title={hiddenCount?'Show hidden terminals':'No hidden terminals'}><LucideIcon icon={Eye} name="eye" /><span class="terminal-dashboard-controls__label">Show hidden terminals</span>{hiddenCount>0&&<span class="terminal-dashboard-controls__count" aria-hidden="true">{hiddenCount}</span>}</wa-button>
    </ToolbarControlGroup>
    <ToolbarControlGroup appearance="borderless" single>
      <wa-dropdown placement="bottom-end"><wa-button slot="trigger" appearance="plain" with-caret aria-label={`Group terminals: ${grouping==='project'?'Project':'None'}`}>Group</wa-button><wa-dropdown-item type="checkbox" checked={grouping==='project'} data-action="set-terminal-grouping" data-terminal-grouping="project"><span slot="icon"><LucideIcon icon={Rows3} name="rows-3" /></span>Project</wa-dropdown-item><wa-dropdown-item type="checkbox" checked={grouping==='flow'} data-action="set-terminal-grouping" data-terminal-grouping="flow"><span slot="icon"><LucideIcon icon={LayoutGrid} name="layout-grid" /></span>None</wa-dropdown-item></wa-dropdown>
    </ToolbarControlGroup>
  </div>;
}

function TerminalTile({ session, magnified = false }: {session:TerminalDashboardSession;magnified?:boolean}) {
  const key = keyFor(session);
  const preview = terminalPreviewText(session.scrollback) || 'Terminal is ready.';
  return <article class="terminal-tile" data-key={key} data-component="terminal-tile" data-terminal-key={key} data-busy={String(session.busy)} data-alive={String(session.alive)} data-magnified={String(magnified)}>
    <button type="button" class="terminal-tile__preview" data-action="magnify-terminal" data-terminal-key={key} aria-label={`${magnified ? 'Restore' : 'Magnify'} ${session.title ?? session.id}`} title={`${magnified ? 'Restore' : 'Magnify'} terminal`}><pre>{preview}</pre></button>
    <footer class="terminal-tile__footer">
      <span class="terminal-tile__state" aria-label={session.busy ? 'Busy' : session.alive ? 'Idle' : 'Exited'}></span>
      <span class="terminal-tile__identity"><strong>{session.projectName}<span aria-hidden="true"> › </span>{session.title ?? session.id}</strong>{session.cwd&&<small>{session.cwd}</small>}</span>
      {session.progress !== undefined && <span class="terminal-tile__progress">{session.progress}%</span>}
      <button type="button" data-action="open-terminal-project" data-terminal-key={key} aria-label={`Open ${session.title ?? session.id} in ${session.projectName}`} title="Open project"><LucideIcon icon={ExternalLink} name="external-link" /></button>
      <button type="button" data-action="dedicate-terminal" data-terminal-key={key} aria-label={`Open ${session.title ?? session.id} as dedicated terminal`} title="Open dedicated terminal"><LucideIcon icon={Maximize2} name="maximize-2" /></button>
      <button type="button" data-action="hide-dashboard-terminal" data-terminal-key={key} aria-label={`Hide ${session.title ?? session.id}`} title="Hide terminal"><LucideIcon icon={EyeOff} name="eye-off" /></button>
    </footer>
  </article>;
}

function Grid({ sessions, layout }: {sessions:TerminalDashboardSession[];layout:ReturnType<typeof terminalGridLayout>}) {
  const style = `--terminal-tile-width:${layout.tileWidth}px;--terminal-tile-height:${layout.tileHeight}px;--terminal-grid-fit:${layout.fit}`;
  return <div class="terminal-grid" data-component="terminal-grid" data-basis={layout.basis} style={style}>{sessions.map(session => <TerminalTile session={session} />)}</div>;
}

export function TerminalDashboard({ groups, width, height, fitAcross, fitHigh, grouping = 'project', magnifiedKey, hiddenKeys = [], loading = false, message = '' }: TerminalDashboardProps) {
  const hidden = new Set(hiddenKeys);
  const visibleGroups = groups.map(group => ({ ...group, sessions: group.sessions.filter(session => !hidden.has(keyFor(session))) })).filter(group => group.sessions.length > 0);
  const sessions = visibleGroups.flatMap(group => group.sessions);
  const layout = terminalGridLayout(width, height, fitAcross, fitHigh);
  const magnified = groups.flatMap(group => group.sessions).find(session => keyFor(session) === magnifiedKey);
  return <section class="terminal-dashboard" data-component="terminal-dashboard" data-basis={layout.basis} data-fit={String(layout.fit)} aria-label="Terminal dashboard">
    <div class="terminal-dashboard__content" data-terminal-grid-measure="true">
      {loading ? <div class="terminal-dashboard__empty" role="status">Loading terminals…</div> : message ? <div class="terminal-dashboard__empty" role="status">{message}</div> : sessions.length === 0 ? <div class="terminal-dashboard__empty"><strong>No active terminals</strong><span>Open a project terminal to add it to this dashboard.</span></div> : grouping === 'flow' ? <Grid sessions={sessions} layout={layout} /> : visibleGroups.map(group => <section class="terminal-dashboard__project" data-key={group.projectId} data-project-id={group.projectId}><h2>{group.projectName}<span>{group.sessions.length}</span></h2><Grid sessions={group.sessions} layout={layout} /></section>)}
    </div>
    <div class="terminal-dashboard__zoom" role="group" aria-label="Terminal tile zoom">
      <button type="button" data-action="zoom-terminal-grid" data-zoom-direction="out" disabled={layout.fit >= layout.max} aria-label={`Zoom out, fit more terminals ${layout.basis}`} title="Zoom out"><LucideIcon icon={Minus} name="minus" /></button>
      <button type="button" data-action="zoom-terminal-grid" data-zoom-direction="in" disabled={layout.fit <= 1} aria-label={`Zoom in, fit fewer terminals ${layout.basis}`} title="Zoom in"><LucideIcon icon={Plus} name="plus" /></button>
    </div>
    {magnified && <div class="terminal-dashboard__magnified" role="dialog" aria-modal="true" aria-label={`Magnified ${magnified.title ?? magnified.id}`}><button type="button" class="terminal-dashboard__magnified-close" data-action="magnify-terminal" data-terminal-key={keyFor(magnified)} aria-label="Restore terminal grid" title="Restore terminal grid"><LucideIcon icon={X} name="x" /></button><TerminalTile session={magnified} magnified /></div>}
  </section>;
}
