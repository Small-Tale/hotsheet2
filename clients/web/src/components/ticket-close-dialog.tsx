import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './ticket-close-dialog.css';

import { CheckCircle2, CopyX, Search } from 'lucide';

import { type DuplicateTarget, duplicateTargetKey, TICKET_CLOSE_REASON_CHOICES, type TicketCloseReason, validateTicketClose } from '../ticket-close';
import { LucideIcon } from './lucide-icon';
import { MenuItem } from './menu-item';
import { Select } from './select';

export interface TicketCloseDialogState {
  source: DuplicateTarget;
  reason: TicketCloseReason;
  query: string;
  candidates: readonly DuplicateTarget[];
  selected?: DuplicateTarget;
  searching?: boolean;
  submitting?: boolean;
  error?: string;
}

export function TicketCloseDialog({ state }: { state?: TicketCloseDialogState }) {
  if (!state) return <></>;
  const duplicate = state.reason === 'duplicate';
  const validation = validateTicketClose(state.reason, state.source, state.selected);
  const candidates = state.candidates.filter(candidate => duplicateTargetKey(candidate) !== duplicateTargetKey(state.source));
  return <wa-dialog class="ticket-close-dialog" open data-component="ticket-close-dialog" label={`Close ${state.source.slug}`}>
    <form data-action="submit-ticket-close" class="ticket-close-dialog__form">
      <p>Record why this ticket is being closed so the outcome remains searchable and unambiguous.</p>
      <Select name="ticket-close-reason" value={state.reason} label="Close as" choices={TICKET_CLOSE_REASON_CHOICES} />
      {duplicate && <section class="ticket-close-dialog__duplicate" aria-label="Duplicate target">
        <wa-input name="ticket-close-target-search" label="Existing ticket" placeholder="Search by ticket number or title" value={state.query}><span slot="start"><LucideIcon icon={Search} name="search" /></span></wa-input>
        {state.selected && <div class="ticket-close-dialog__selected" role="status"><LucideIcon icon={CheckCircle2} name="check-circle-2" /><span><strong>{state.selected.slug}<small>{state.selected.projectName}</small></strong><span>{state.selected.title}</span></span><wa-button type="button" appearance="plain" size="small" data-action="clear-ticket-close-target">Change</wa-button></div>}
        {!state.selected && state.query.trim() && <div class="ticket-close-dialog__results" aria-label="Matching tickets" aria-busy={String(Boolean(state.searching))}>
          {candidates.map(candidate => <MenuItem action="select-ticket-close-target" itemId={duplicateTargetKey(candidate)} icon={<LucideIcon icon={CopyX} name="copy-x" />} label={<><strong>{candidate.slug}<small>{candidate.projectName}</small></strong><span>{candidate.title}</span></>} multiline />)}
          {!state.searching && candidates.length === 0 && <p>No matching tickets.</p>}
        </div>}
        <p class="ticket-close-dialog__hint">The selected ticket becomes the canonical target. This relationship is stored as structured duplicate metadata.</p>
      </section>}
      <p class="ticket-close-dialog__error" role="alert">{state.error || validation}</p>
      <div slot="footer" class="ticket-close-dialog__actions">
        <wa-button type="button" data-action="cancel-ticket-close" appearance="outlined" disabled={state.submitting}>Cancel</wa-button>
        <wa-button type="submit" variant="brand" disabled={Boolean(validation) || state.submitting}>{state.submitting ? 'Closing…' : duplicate ? 'Mark as duplicate' : 'Close ticket'}</wa-button>
      </div>
    </form>
  </wa-dialog>;
}
