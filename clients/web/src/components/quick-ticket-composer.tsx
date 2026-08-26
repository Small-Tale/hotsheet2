import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import { Plus, X } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './quick-ticket-composer.css';

export interface QuickTicketComposerProps {
  expanded?: boolean;
  title?: string;
  category?: string;
  providerName?: string;
  canCreate?: boolean;
}

export function QuickTicketComposer({ expanded = false, title = '', category = 'task', providerName = 'Hot Sheet', canCreate = true }: QuickTicketComposerProps) {
  if (!expanded) return <button type="button" class="quick-ticket-composer__launcher" data-component="quick-ticket-composer" data-action="expand-ticket-composer"><LucideIcon icon={Plus} name="plus" />New ticket…</button>;
  return <form class="quick-ticket-composer" data-component="quick-ticket-composer" data-action="create-ticket-form">
    <wa-input name="new-ticket-title" label="Ticket title" value={title} autofocus required></wa-input>
    <wa-select name="new-ticket-category" label="Category" value={category}><wa-option value="task">Task</wa-option><wa-option value="feature">Feature</wa-option><wa-option value="bug">Bug</wa-option><wa-option value="investigation">Investigation</wa-option></wa-select>
    <div class="quick-ticket-composer__footer">
      <span>Creating in {providerName}</span>
      <div><wa-button type="button" appearance="plain" data-action="cancel-ticket-composer"><LucideIcon icon={X} name="x" />Cancel</wa-button><wa-button type="submit" appearance="accent" disabled={!canCreate}>Create ticket</wa-button></div>
    </div>
    {!canCreate && <p class="quick-ticket-composer__notice" role="status">This ticket provider does not support creating tickets.</p>}
  </form>;
}
