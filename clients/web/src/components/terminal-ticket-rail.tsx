import './terminal-ticket-rail.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { PanelRightClose } from 'lucide';

import { ContentTransition, type ContentTransitionDirection } from './content-transition';
import { LucideIcon } from './lucide-icon';
import { PageHeader } from './page-header';
import { Select } from './select';
import { Toolbar } from './toolbar';
import { ToolbarControlGroup } from './toolbar-control-group';

export interface TerminalTicketRailProps {
  projects: readonly { id:string;name:string }[];
  selectedProjectId: string;
  views?: readonly { id:string;label:string }[];
  selectedViewId?: string;
  controls: SafeHtml;
  content: SafeHtml;
  inspector: SafeHtml;
  active: 'root'|'ticket';
  direction?: ContentTransitionDirection;
  title?: string;
  action?: SafeHtml;
}

export function TerminalTicketRail({projects,selectedProjectId,views=[],selectedViewId='all',controls,content,inspector,active,direction='forward',title='Queue',action}:TerminalTicketRailProps){
  const heading=views.length?<Select className="terminal-ticket-rail__view" name="terminal-rail-view" value={selectedViewId} ariaLabel="Ticket rail view" fitMenu choices={views.map(view=>({value:view.id,label:view.label}))} renderSelected={choice=><span>{choice.label}</span>}/>:title;
  const root=<section class="terminal-ticket-rail__root" aria-label="Project tickets">
    <Toolbar className="terminal-ticket-rail__project" divider={false} leading={<Select name="terminal-rail-project" value={selectedProjectId} ariaLabel="Ticket rail project" fitMenu choices={projects.map(project=>({value:project.id,label:project.name}))} renderSelected={choice=><span>{choice.label}</span>}/>} trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" data-action="close-ticket-inspector" aria-label="Hide ticket rail" title="Hide ticket rail"><LucideIcon icon={PanelRightClose} name="panel-right-close"/></button></ToolbarControlGroup>}/>
    <div class="terminal-ticket-rail__controls">{controls}</div>
    <PageHeader title={heading} action={action}/>
    <div class="terminal-ticket-rail__content">{content}</div>
  </section>;
  return <aside class="terminal-ticket-rail" data-component="terminal-ticket-rail" data-screen={active} aria-label="Ticket rail">
    <ContentTransition active={active==='ticket'?'b':'a'} direction={direction} a={root} b={inspector} label="Ticket navigation"/>
  </aside>;
}
