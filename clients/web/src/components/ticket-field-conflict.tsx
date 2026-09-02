import '@awesome.me/webawesome/dist/components/button/button.js';
import './ticket-field-conflict.css';

import { GitMerge, RotateCcw } from 'lucide';

import type { TicketFieldConflict as TicketFieldConflictState } from '../ticket-field-reconciliation';
import { LucideIcon } from './lucide-icon';

export interface TicketFieldConflictProps {
  conflict: TicketFieldConflictState;
  resolution: string;
}

export function TicketFieldConflict({ conflict, resolution }: TicketFieldConflictProps) {
  return <section class="ticket-field-conflict" data-component="ticket-field-conflict" data-conflict-field={conflict.key} aria-label={`Resolve ${conflict.label.toLocaleLowerCase()} conflict`}>
    <header>
      <LucideIcon icon={GitMerge} name="git-merge" />
      <span><strong>Resolve {conflict.label.toLocaleLowerCase()} conflict</strong><small>This field changed elsewhere while you were editing it.</small></span>
    </header>
    <div class="ticket-field-conflict__versions" aria-label="Conflicting versions">
      <label><span>Their latest version</span><output>{conflict.theirs || 'Empty'}</output></label>
      <label><span>Your version</span><output>{conflict.mine || 'Empty'}</output></label>
    </div>
    <label class="ticket-field-conflict__merge"><span>Merged value</span><textarea name="ticket-conflict-resolution" aria-label={`Merged ${conflict.label.toLocaleLowerCase()}`}>{resolution}</textarea></label>
    <footer>
      <button type="button" data-action="accept-remote-ticket-field"><LucideIcon icon={RotateCcw} name="rotate-ccw" />Use theirs</button>
      <button type="button" data-action="apply-ticket-field-merge"><LucideIcon icon={GitMerge} name="git-merge" />Apply merged value</button>
    </footer>
  </section>;
}
