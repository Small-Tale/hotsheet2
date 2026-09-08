import './terminal-ticket-rail.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';

import { ContentTransition, type ContentTransitionDirection } from './content-transition';
import { PageHeader } from './page-header';
import { Select } from './select';
import { Toolbar } from './toolbar';

export interface TerminalTicketRailProps {
  projects: readonly { id:string;name:string }[];
  selectedProjectId: string;
  controls: SafeHtml;
  content: SafeHtml;
  inspector: SafeHtml;
  active: 'root'|'ticket';
  direction?: ContentTransitionDirection;
  title?: string;
  action?: SafeHtml;
}

export function TerminalTicketRail({projects,selectedProjectId,controls,content,inspector,active,direction='forward',title='Queue',action}:TerminalTicketRailProps){
  const root=<section class="terminal-ticket-rail__root" aria-label="Project tickets">
    <Toolbar className="terminal-ticket-rail__project" divider={false} leading={<Select name="terminal-rail-project" value={selectedProjectId} ariaLabel="Ticket rail project" fitMenu choices={projects.map(project=>({value:project.id,label:project.name}))}/>}/>
    <div class="terminal-ticket-rail__controls">{controls}</div>
    <PageHeader title={title} action={action}/>
    <div class="terminal-ticket-rail__content">{content}</div>
  </section>;
  return <aside class="terminal-ticket-rail" data-component="terminal-ticket-rail" data-screen={active} aria-label="Ticket rail">
    <ContentTransition active={active==='ticket'?'b':'a'} direction={direction} a={root} b={inspector} label="Ticket navigation"/>
  </aside>;
}
