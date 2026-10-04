import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import type { ClaimEtaPresentation } from '../active-ticket-work';
import { CATEGORY_COLORS, CATEGORY_ICONS } from '../components/category-presentation';
import type { TicketStatus } from '../components/status-badge';
import { type TicketPriority, TicketRow } from '../components/ticket-row';
import { syncSettingsControls } from './settings-controls';

/** Claim ETA states the demo exposes (HS2-XQMDQB). */
export type TicketRowClaimEta = 'none' | 'estimate' | 'overrun';
export const TICKET_ROW_CLAIM_ETA: Record<Exclude<TicketRowClaimEta, 'none'>, ClaimEtaPresentation> = {
  estimate: { kind: 'estimate', percent: 25, label: '~45m left', title: 'Estimated to finish in about 45 minutes' },
  overrun: { kind: 'overrun', label: 'Soon', title: 'Past its estimate by about 10 minutes' },
};

/** Completion confidence choices the demo exposes (HS2-A0Q6G6); `none` is unscored. */
const TICKET_ROW_CONFIDENCE_CHOICES: SelectChoice[] = [
  { value: 'none', label: 'Unscored' },
  { value: '94', label: '94% (fully verified)' },
  { value: '82', label: '82% (minor assumptions)' },
  { value: '55', label: '55% (partially verified)' },
  { value: '25', label: '25% (largely unverified)' },
];

export const ticketRowSettings = {
  title: signal('Build the first client ticket list'),
  status: signal<TicketStatus>('started'),
  priority: signal<TicketPriority>('high'),
  category: signal('feature'),
  tags: signal('client, ux'),
  upNext: signal(true),
  blocked: signal(false),
  needsReview: signal(false),
  feedbackNeeded: signal(false),
  selected: signal(false),
  busy: signal(true),
  claimEta: signal<TicketRowClaimEta>('estimate'),
  /** Derived completion confidence; shown only for completed/verified rows (HS2-A0Q6G6). */
  confidence: signal<string>('none'),
  categoryIcon: signal('sparkles'),
  categoryColor: signal('#3b82f6'),
  agentName: signal('Claude'),
  updatedLabel: signal('1h ago'),
  event: signal('No actions yet'),
};

export function resetTicketRowDemo(root?: ParentNode): void {
  ticketRowSettings.title.value = 'Build the first client ticket list';
  ticketRowSettings.status.value = 'started';
  ticketRowSettings.priority.value = 'high';
  ticketRowSettings.category.value = 'feature';
  ticketRowSettings.tags.value = 'client, ux';
  ticketRowSettings.upNext.value = true;
  ticketRowSettings.blocked.value = false;
  ticketRowSettings.needsReview.value = false;
  ticketRowSettings.feedbackNeeded.value = false;
  ticketRowSettings.selected.value = false;
  ticketRowSettings.busy.value = true;
  ticketRowSettings.claimEta.value = 'estimate';
  ticketRowSettings.confidence.value = 'none';
  ticketRowSettings.categoryIcon.value = 'sparkles';
  ticketRowSettings.categoryColor.value = '#3b82f6';
  ticketRowSettings.agentName.value = 'Claude';
  ticketRowSettings.updatedLabel.value = '1h ago';
  ticketRowSettings.event.value = 'No actions yet';
  if (root)
    syncSettingsControls(root, 'ticket-list-row', {
      values: {
        title: ticketRowSettings.title.value,
        status: ticketRowSettings.status.value,
        priority: ticketRowSettings.priority.value,
        category: ticketRowSettings.category.value,
        tags: ticketRowSettings.tags.value,
        'category-icon': ticketRowSettings.categoryIcon.value,
        'claim-eta': ticketRowSettings.claimEta.value,
        confidence: ticketRowSettings.confidence.value,
        'category-color': ticketRowSettings.categoryColor.value,
        agent: ticketRowSettings.agentName.value,
        updated: ticketRowSettings.updatedLabel.value,
      },
      checked: {
        'up-next': ticketRowSettings.upNext.value,
        blocked: ticketRowSettings.blocked.value,
        'needs-review': ticketRowSettings.needsReview.value,
        'feedback-needed': ticketRowSettings.feedbackNeeded.value,
        selected: ticketRowSettings.selected.value,
        busy: ticketRowSettings.busy.value,
      },
    });
}

export function TicketRowDemo() {
  return (
    <section class="component-stage component-stage--row" aria-label="TicketRow demo">
      <div class="component-stage__canvas component-stage__canvas--row" role="listbox" aria-label="Example ticket list">
        {TicketRow({
          slug: 'HS2-D3M0',
          title: ticketRowSettings.title.value,
          status: ticketRowSettings.status.value,
          priority: ticketRowSettings.priority.value,
          category: ticketRowSettings.category.value,
          tags: ticketRowSettings.tags.value.split(','),
          upNext: ticketRowSettings.upNext.value,
          blocked: ticketRowSettings.blocked.value,
          needsReview: ticketRowSettings.needsReview.value,
          feedbackNeeded: ticketRowSettings.feedbackNeeded.value,
          selected: ticketRowSettings.selected.value,
          busy: ticketRowSettings.busy.value,
          claimEta:
            ticketRowSettings.claimEta.value === 'none'
              ? undefined
              : TICKET_ROW_CLAIM_ETA[ticketRowSettings.claimEta.value],
          categoryIcon: ticketRowSettings.categoryIcon.value,
          categoryColor: ticketRowSettings.categoryColor.value,
          agentName: ticketRowSettings.agentName.value,
          updatedLabel: ticketRowSettings.updatedLabel.value,
          latestConfidence:
            ticketRowSettings.confidence.value === 'none' ? undefined : Number(ticketRowSettings.confidence.value),
        })}
      </div>
      <p class="component-stage__event" aria-live="polite">
        {ticketRowSettings.event}
      </p>
      <p class="component-stage__guidance">
        The full row is the selection target. Metadata stays scannable while tags and transient AI activity remain
        secondary.
      </p>
    </section>
  );
}

export function TicketRowSettings() {
  return (
    <form class="settings-form" data-settings="ticket-list-row">
      <wa-input name="title" label="Title" value={ticketRowSettings.title.value} />
      <Select
        name="status"
        label="Status"
        value={ticketRowSettings.status.value}
        choices={['not_started', 'started', 'completed', 'verified', 'backlog'].map((value) => ({
          value,
          label: value.replace('_', ' '),
        }))}
      />
      <Select
        name="priority"
        label="Priority"
        value={ticketRowSettings.priority.value}
        choices={(['low', 'default', 'high', 'urgent'] as const).map((value) => ({ value, label: value }))}
      />
      <wa-input name="category" label="Category" value={ticketRowSettings.category.value} />
      <Select
        name="category-icon"
        label="Category icon"
        value={ticketRowSettings.categoryIcon.value}
        choices={CATEGORY_ICONS.map((option) => ({ value: option.value, label: option.label }))}
      />
      <Select
        name="category-color"
        label="Category icon color"
        value={ticketRowSettings.categoryColor.value}
        choices={CATEGORY_COLORS.map((option): SelectChoice => ({ value: option.value, label: option.label }))}
      />
      <wa-input name="tags" label="Tags (comma separated)" value={ticketRowSettings.tags.value} />
      <wa-input name="agent" label="Active agent" value={ticketRowSettings.agentName.value} />
      <wa-input name="updated" label="Updated label" value={ticketRowSettings.updatedLabel.value} />
      <wa-checkbox name="up-next" checked={ticketRowSettings.upNext.value}>
        Up Next
      </wa-checkbox>
      <wa-checkbox name="blocked" checked={ticketRowSettings.blocked.value}>
        Blocked
      </wa-checkbox>
      <wa-checkbox name="needs-review" checked={ticketRowSettings.needsReview.value}>
        Needs review
      </wa-checkbox>
      <wa-checkbox name="feedback-needed" checked={ticketRowSettings.feedbackNeeded.value}>
        Feedback needed
      </wa-checkbox>
      <wa-checkbox name="selected" checked={ticketRowSettings.selected.value}>
        Selected
      </wa-checkbox>
      <wa-checkbox name="busy" checked={ticketRowSettings.busy.value}>
        AI working
      </wa-checkbox>
      <Select
        name="claim-eta"
        label="Claim ETA (shown while AI working)"
        value={ticketRowSettings.claimEta.value}
        choices={[
          { value: 'none', label: 'No estimate' },
          { value: 'estimate', label: 'On track (~45m left)' },
          { value: 'overrun', label: 'Past its estimate (Soon)' },
        ]}
      />
      <Select
        name="confidence"
        label="Completion confidence (shown when completed or verified)"
        value={ticketRowSettings.confidence.value}
        choices={TICKET_ROW_CONFIDENCE_CHOICES}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}
