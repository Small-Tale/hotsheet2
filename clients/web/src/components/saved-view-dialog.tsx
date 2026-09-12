import './saved-view-dialog.css';
import './workspace-header.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { Search, X } from 'lucide';

import {inlineSearchParts,type InlineSearchToken,orderedSearchText} from '../inline-search';

export interface SavedViewDialogProps {
  open: boolean;
  mode?: 'create' | 'rename';
  name: string;
  query: string;
  queryTokens?: readonly InlineSearchToken[];
  busy?: boolean;
  error?: string;
}

export function SavedViewDialog({ open, mode = 'create', name, query, queryTokens=[], busy = false, error = '' }: SavedViewDialogProps) {
  const rename=mode==='rename',title=rename?'Edit View':'Create View',queryParts=inlineSearchParts(query,queryTokens),queryValue=orderedSearchText(query,queryTokens,()=>true);
  return <wa-dialog class="saved-view-dialog" data-component="saved-view-dialog" data-mode={mode} label={title} aria-label={title} open={open || undefined}>
    <form data-action="save-saved-view" class="saved-view-dialog__form">
      <p>{rename?'Change the shared view name or search query.':'Save a search as a project view. Everyone using this ticket store will see it.'}</p>
      <wa-input name="saved-view-name" label="View name" value={name} required autofocus disabled={busy || undefined}></wa-input>
      <label class="saved-view-dialog__query"><span>Search query <sup aria-hidden="true">*</sup></span><ToolbarControlGroup className="workspace-header__search-group saved-view-dialog__query-field" expanded><div class="workspace-header__search-editor"><span class="workspace-header__search-icon" aria-hidden="true"><LucideIcon icon={Search} name="search" /></span><div class="workspace-header__search" data-key={`saved-view-query:${queryTokens.map(token=>token.raw).join('|')}`} data-morph-skip data-saved-view-query="true" role="textbox" aria-label="Search query" aria-multiline="true" contenteditable={busy?'false':'true'} data-placeholder="Search tickets" spellcheck="false">{queryParts.map(part=>part.kind==='text'?<span data-search-text="true" data-empty={String(part.value.length===0)}>{part.value||(queryTokens.length?'\u200b':'')}</span>:<span class="workspace-header__search-token" contenteditable="false" data-component="filter-chip" data-token-raw={part.token.raw} title="Double-click to edit"><button type="button" class="workspace-header__search-token-edit" data-action="edit-saved-view-query-token" data-token-raw={part.token.raw} aria-label={`Edit ${part.token.label.replace(/^tag:/,'tag ')}`}>{part.token.label}</button><button type="button" data-action="remove-saved-view-query-token" data-token-raw={part.token.raw} aria-label={`Remove ${part.token.label.replace(/^tag:/,'tag ')}`}><LucideIcon icon={X} name="x"/></button></span>)}</div>{(query||queryTokens.length>0)&&<span class="workspace-header__search-end"><button type="button" class="workspace-header__search-clear" data-action="clear-saved-view-query" aria-label="Clear search query" title="Clear search query"><LucideIcon icon={X} name="x" /></button></span>}</div></ToolbarControlGroup><input type="hidden" name="saved-view-query" value={queryValue}/><small>Use the same words, fields, operators, and filter chips as ticket search.</small></label>
      {error && <p class="saved-view-dialog__error" role="alert">{error}</p>}
      <footer>
        <wa-button appearance="plain" type="button" data-action="cancel-saved-view" disabled={busy || undefined}>Cancel</wa-button>
        <wa-button appearance="accent" type="submit" disabled={busy || undefined}>{busy ? rename?'Saving…':'Creating…' : title}</wa-button>
      </footer>
    </form>
  </wa-dialog>;
}

export function SavedViewDeleteDialog({open,name,busy=false,error=''}:{open:boolean;name:string;busy?:boolean;error?:string}){
  return <wa-dialog class="saved-view-dialog" data-component="saved-view-delete-dialog" label="Delete View?" aria-label="Delete View?" open={open||undefined}>
    <div class="saved-view-dialog__form"><p>Delete <strong>{name}</strong> for everyone using this ticket store? Tickets are not affected.</p>{error&&<p class="saved-view-dialog__error" role="alert">{error}</p>}<footer><wa-button appearance="plain" type="button" data-action="cancel-delete-saved-view" disabled={busy||undefined}>Cancel</wa-button><wa-button variant="danger" type="button" data-action="confirm-delete-saved-view" disabled={busy||undefined}>{busy?'Deleting…':'Delete View'}</wa-button></footer></div>
  </wa-dialog>
}
