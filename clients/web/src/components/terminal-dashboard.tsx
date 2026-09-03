import './terminal-dashboard.css';

import { ExternalLink, Eye, EyeOff, LayoutGrid, Maximize2, Minus, Plus, Rows3, X } from 'lucide';

import { terminalGridLayout, terminalPreviewText } from '../terminal-grid-layout';
import { LucideIcon } from './lucide-icon';

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

function TerminalTile({ session, magnified = false }: {session:TerminalDashboardSession;magnified?:boolean}) {
  const key = keyFor(session);
  const preview = terminalPreviewText(session.scrollback) || 'Terminal is ready.';
  return <article class="terminal-tile" data-key={key} data-component="terminal-tile" data-terminal-key={key} data-busy={String(session.busy)} data-alive={String(session.alive)} data-magnified={String(magnified)}>
    <button type="button" class="terminal-tile__preview" data-action="magnify-terminal" data-terminal-key={key} aria-label={`${magnified ? 'Restore' : 'Magnify'} ${session.title ?? session.id}`} title={`${magnified ? 'Restore' : 'Magnify'} terminal`}><pre>{preview}</pre></button>
    <footer class="terminal-tile__footer">
      <span class="terminal-tile__state" aria-label={session.busy ? 'Busy' : session.alive ? 'Idle' : 'Exited'}></span>
      <span class="terminal-tile__identity"><strong>{session.title ?? session.id}</strong><small>{session.projectName}{session.cwd ? ` · ${session.cwd}` : ''}</small></span>
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
    <header class="terminal-dashboard__toolbar">
      <div class="terminal-dashboard__grouping" role="group" aria-label="Terminal grouping">
        <button type="button" data-action="set-terminal-grouping" data-terminal-grouping="project" aria-pressed={String(grouping === 'project')} title="Group by project"><LucideIcon icon={Rows3} name="rows-3" /><span>Projects</span></button>
        <button type="button" data-action="set-terminal-grouping" data-terminal-grouping="flow" aria-pressed={String(grouping === 'flow')} title="Flow all terminals"><LucideIcon icon={LayoutGrid} name="layout-grid" /><span>Flow</span></button>
      </div>
      {hidden.size > 0 && <button type="button" class="terminal-dashboard__show-hidden" data-action="show-hidden-terminals"><LucideIcon icon={Eye} name="eye" /><span>Show hidden ({hidden.size})</span></button>}
      <span class="terminal-dashboard__fit" aria-live="polite">{layout.fit} {layout.basis}</span>
    </header>
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
