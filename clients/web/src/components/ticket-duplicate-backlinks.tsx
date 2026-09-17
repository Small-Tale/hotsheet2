import './ticket-duplicate-backlinks.css';

import { ListHeader } from '@kerfjs/ui/list-header';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { CopyX } from 'lucide';

import type { DuplicateBacklink } from '../api';

export interface DuplicateTargetSummary { id:string; projectName:string; slug:string; title:string }

function DuplicateTicketItem({id,projectName,slug,title,accessibleLabel}:{id:string;projectName:string;slug:string;title:string;accessibleLabel:string}){
  return <ListItem action="open-duplicate-target" itemId={id} icon={<LucideIcon icon={CopyX} name="copy-x"/>} label={<><strong>{projectName} · {slug}</strong><span>{title}</span></>} accessibleLabel={accessibleLabel} multiline/>;
}

export function TicketDuplicateTarget({target}:{target:DuplicateTargetSummary}){
  return <section class="ticket-duplicate-backlinks" data-component="ticket-duplicate-target" aria-label="Duplicate target"><ListHeader label="Duplicate of"/><div class="ticket-duplicate-backlinks__items"><DuplicateTicketItem id={target.id} projectName={target.projectName} slug={target.slug} title={target.title} accessibleLabel={`Open duplicate target ${target.slug} from ${target.projectName}`}/></div></section>;
}

export function TicketDuplicateBacklinks({ backlinks, inaccessibleProjects = [] }: { backlinks: readonly DuplicateBacklink[]; inaccessibleProjects?: readonly string[] }) {
  if (backlinks.length === 0 && inaccessibleProjects.length === 0) return null;
  return <section class="ticket-duplicate-backlinks" data-component="ticket-duplicate-backlinks" aria-label="Duplicate backlinks">
    <ListHeader label="Duplicates" count={backlinks.length} countLabel={`${backlinks.length} ${backlinks.length===1?'duplicate':'duplicates'}`}/>
    {backlinks.length > 0 && <div class="ticket-duplicate-backlinks__items">{backlinks.map(backlink => <DuplicateTicketItem id={backlink.reference} projectName={backlink.project_name} slug={backlink.slug} title={backlink.title} accessibleLabel={`Open duplicate ${backlink.slug} from ${backlink.project_name}`}/>)}</div>}
    {inaccessibleProjects.length > 0 && <p class="ticket-duplicate-backlinks__warning" role="status">Could not check {inaccessibleProjects.join(', ')} for additional duplicates.</p>}
  </section>;
}
