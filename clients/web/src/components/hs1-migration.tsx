import './hs1-migration.css';

import { ArchiveRestore, Trash2 } from 'lucide';

import { LucideIcon } from './lucide-icon';

export interface Hs1MigrationDialogProps {
  projectName:string;
  defaultStore:string;
  open:boolean;
  busy:boolean;
  error?:string;
}

export function Hs1MigrationDialog({projectName,defaultStore,open,busy,error}:Hs1MigrationDialogProps){
  return <wa-dialog data-component="hs1-migration-dialog" label="Import Hot Sheet 1 project" open={open}>
    <form class="hs1-migration-dialog" data-action="import-hs1-project">
      <div class="hs1-migration-dialog__intro"><LucideIcon icon={ArchiveRestore} name="archive-restore"/><div><strong>Hot Sheet 1 data found in {projectName}</strong><p>Import its tickets, notes, attachments, settings, and AI-tool setup into Hot Sheet 2. Your old data stays in place until the new ticket repository is backed up.</p></div></div>
      <div class="hs1-migration-dialog__destination"><wa-input name="hs1-ticket-store" label="Ticket repository folder" value={defaultStore} required disabled={busy}></wa-input><wa-button appearance="outlined" type="button" data-action="browse-hs1-ticket-store" disabled={busy}>Choose…</wa-button></div>
      {busy&&<p class="hs1-migration-dialog__progress" role="status">Importing the project and configuring detected AI tools…</p>}
      {error&&<p class="hs1-migration-dialog__error" role="alert">{error}</p>}
      <footer><wa-button appearance="plain" type="button" data-action="dismiss-hs1-migration" disabled={busy}>Not now</wa-button><wa-button appearance="accent" type="submit" disabled={busy}>{busy?'Importing…':'Import project'}</wa-button></footer>
    </form>
  </wa-dialog>;
}

export function Hs1CleanupBanner(){return <section class="hs1-cleanup-banner" data-component="hs1-cleanup-banner" role="status"><LucideIcon icon={Trash2} name="trash-2"/><div><strong>Hot Sheet 1 import is safely backed up</strong><span>The old local Hot Sheet 1 files can now be removed.</span></div><button type="button" data-action="remove-hs1-data">Delete old files…</button></section>}
