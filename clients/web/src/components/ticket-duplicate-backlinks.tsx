import './ticket-duplicate-backlinks.css';

import { CopyX } from 'lucide';

import type { DuplicateBacklink } from '../api';
import { LucideIcon } from './lucide-icon';
import { MenuHeader } from './menu-header';
import { MenuItem } from './menu-item';

export function TicketDuplicateBacklinks({ backlinks, inaccessibleProjects = [] }: { backlinks: readonly DuplicateBacklink[]; inaccessibleProjects?: readonly string[] }) {
  if (backlinks.length === 0 && inaccessibleProjects.length === 0) return null;
  return <section class="ticket-duplicate-backlinks" data-component="ticket-duplicate-backlinks" aria-label="Duplicate backlinks">
    <MenuHeader label={`Duplicates ${backlinks.length}`}/>
    {backlinks.length > 0 && <div class="ticket-duplicate-backlinks__items">{backlinks.map(backlink => <MenuItem action="open-duplicate-target" itemId={backlink.reference} icon={<LucideIcon icon={CopyX} name="copy-x"/>} label={<><strong>{backlink.project_name} · {backlink.slug}</strong><span>{backlink.title}</span></>} accessibleLabel={`Open duplicate ${backlink.slug} from ${backlink.project_name}`} multiline/>)}</div>}
    {inaccessibleProjects.length > 0 && <p class="ticket-duplicate-backlinks__warning" role="status">Could not check {inaccessibleProjects.join(', ')} for additional duplicates.</p>}
  </section>;
}
