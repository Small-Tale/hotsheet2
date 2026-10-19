import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { ClaimEtaPresentation, PhaseEstimatePresentation } from '../active-ticket-work';
import type { StartedPhase } from '../api';
import { CATEGORY_COLORS, CATEGORY_ICONS } from '../components/category-presentation';
import { CorruptTicketRow } from '../components/corrupt-ticket-row';
import type { TicketStatus } from '../components/status-badge';
import { type TicketPriority, TicketRow } from '../components/ticket-row';
import { NAVIGATION_AND_TABS_ACTIONS } from '../interaction-attrs/navigation-and-tabs';
import { DemoEvent } from './components/demo-event';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** Claim ETA states the demo exposes (HS2-XQMDQB). */
export type TicketRowClaimEta = 'none' | 'estimate' | 'overrun';
export const TICKET_ROW_CLAIM_ETA: Record<Exclude<TicketRowClaimEta, 'none'>, ClaimEtaPresentation> = {
  estimate: { kind: 'estimate', percent: 25, label: '~45m left', title: 'Estimated to finish in about 45 minutes' },
  overrun: { kind: 'overrun', label: 'Soon', title: 'Past its estimate by about 10 minutes' },
};

/** Testing-phase estimate states the demo exposes (HS2-YDGM04); shown in a Started testing phase. */
export type TicketRowPhaseEstimate = 'none' | 'estimate' | 'overrun';
export const TICKET_ROW_PHASE_ESTIMATE: Record<Exclude<TicketRowPhaseEstimate, 'none'>, PhaseEstimatePresentation> = {
  estimate: { kind: 'estimate', label: '~6m left', title: 'This phase typically takes about 12m' },
  overrun: { kind: 'overrun', label: 'Soon', title: "Past this phase's typical duration (about 12m)" },
};

/** Completion confidence choices the demo exposes (HS2-A0Q6G6); `none` is unscored. */
const TICKET_ROW_CONFIDENCE_CHOICES: SelectChoice[] = [
  { value: 'none', label: 'Unscored' },
  { value: '94', label: '94% (Fully Verified)' },
  { value: '82', label: '82% (Minor Assumptions)' },
  { value: '55', label: '55% (Partially Verified)' },
  { value: '25', label: '25% (Largely Unverified)' },
];

export const ticketRowSettings = {
  title: signal('Build the first client ticket list'),
  status: signal<TicketStatus>('started'),
  startedPhase: signal<'' | StartedPhase>(''),
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
  phaseEstimate: signal<TicketRowPhaseEstimate>('estimate'),
  /** Derived completion confidence; shown only for completed/verified rows (HS2-A0Q6G6). */
  confidence: signal<string>('none'),
  categoryIcon: signal('sparkles'),
  categoryColor: signal('#3b82f6'),
  showSourceIcon: signal(true),
  agentName: signal('Claude'),
  updatedLabel: signal('1h ago'),
  event: signal('No actions yet'),
};

export function resetTicketRowDemo(root?: ParentNode): void {
  ticketRowSettings.title.value = 'Build the first client ticket list';
  ticketRowSettings.status.value = 'started';
  ticketRowSettings.startedPhase.value = '';
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
  ticketRowSettings.phaseEstimate.value = 'estimate';
  ticketRowSettings.confidence.value = 'none';
  ticketRowSettings.categoryIcon.value = 'sparkles';
  ticketRowSettings.categoryColor.value = '#3b82f6';
  ticketRowSettings.showSourceIcon.value = true;
  ticketRowSettings.agentName.value = 'Claude';
  ticketRowSettings.updatedLabel.value = '1h ago';
  ticketRowSettings.event.value = 'No actions yet';
  if (root)
    syncSettingsControls(root, 'ticket-list-row', {
      values: {
        title: ticketRowSettings.title.value,
        status: ticketRowSettings.status.value,
        'started-phase': ticketRowSettings.startedPhase.value,
        priority: ticketRowSettings.priority.value,
        category: ticketRowSettings.category.value,
        tags: ticketRowSettings.tags.value,
        'category-icon': ticketRowSettings.categoryIcon.value,
        'claim-eta': ticketRowSettings.claimEta.value,
        'phase-estimate': ticketRowSettings.phaseEstimate.value,
        confidence: ticketRowSettings.confidence.value,
        'category-color': ticketRowSettings.categoryColor.value,
        agent: ticketRowSettings.agentName.value,
      },
      checked: {
        'up-next': ticketRowSettings.upNext.value,
        blocked: ticketRowSettings.blocked.value,
        'needs-review': ticketRowSettings.needsReview.value,
        'feedback-needed': ticketRowSettings.feedbackNeeded.value,
        selected: ticketRowSettings.selected.value,
        busy: ticketRowSettings.busy.value,
        'show-source-icon': ticketRowSettings.showSourceIcon.value,
      },
    });
}

export const TICKET_ROW_DEMO_SLUG = 'Small-Tale/hotsheet2#5';

/** The danger (diagnostic) presentation of the shared row shell, as CorruptTicketRow renders it. */
const TICKET_ROW_DEMO_CORRUPT = {
  store: 'local',
  store_path: '/work/demo.hs2',
  path: '/work/demo.hs2/tickets/01/01M1.md',
  id: '01M1',
  slug: 'HS2-BROKEN',
  error: 'unsupported content follows the bounded Notes section',
};

export function TicketRowDemo() {
  return (
    <>
      <CatalogExample
        label="Interactive"
        note="The full row is the selection target. Metadata stays scannable while tags and transient AI activity remain secondary."
        viewport={{ width: 'full' }}
      >
        <div role="listbox" aria-label="Example ticket list">
          {TicketRow({
            slug: TICKET_ROW_DEMO_SLUG,
            title: ticketRowSettings.title.value,
            status: ticketRowSettings.status.value,
            startedPhase: ticketRowSettings.startedPhase.value || undefined,
            priority: ticketRowSettings.priority.value,
            category: ticketRowSettings.category.value,
            source: { provider: 'github', name: 'Product issues', color: '#3b82f6' },
            showSourceIcon: ticketRowSettings.showSourceIcon.value,
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
            phaseEstimate:
              ticketRowSettings.phaseEstimate.value === 'none'
                ? undefined
                : TICKET_ROW_PHASE_ESTIMATE[ticketRowSettings.phaseEstimate.value],
            categoryIcon: ticketRowSettings.categoryIcon.value,
            categoryColor: ticketRowSettings.categoryColor.value,
            agentName: ticketRowSettings.agentName.value,
            updatedLabel: ticketRowSettings.updatedLabel.value,
            latestConfidence:
              ticketRowSettings.confidence.value === 'none' ? undefined : Number(ticketRowSettings.confidence.value),
          })}
        </div>
      </CatalogExample>
      {/* The shared row shell's danger presentation (HS2-1FH8BG): a danger rail and a selection wiggle
          around CorruptTicketRow's diagnostic content; Selected drives both rows. */}
      <CatalogExample
        label="Danger Presentation"
        viewport={{ width: 'full' }}
        rootAttributes={{ 'data-ticket-row-demo-danger': '' }}
      >
        {CorruptTicketRow({
          ticket: TICKET_ROW_DEMO_CORRUPT,
          selected: ticketRowSettings.selected.value,
          listEdge: 'only',
        })}
      </CatalogExample>
      <DemoEvent>{ticketRowSettings.event.value}</DemoEvent>
    </>
  );
}

export function TicketRowSettings() {
  return (
    <DemoSettingsForm settings="ticket-list-row">
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
        name="started-phase"
        label="Started Phase"
        value={ticketRowSettings.startedPhase.value}
        choices={[
          { value: '', label: 'None' },
          { value: 'analyzing', label: 'Analyzing' },
          { value: 'planning', label: 'Planning' },
          { value: 'working', label: 'Working' },
          { value: 'initial_testing', label: 'Initial Testing' },
          { value: 'integrating', label: 'Integrating' },
          { value: 'final_testing', label: 'Final Testing' },
        ]}
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
        label="Category Icon"
        value={ticketRowSettings.categoryIcon.value}
        choices={CATEGORY_ICONS.map((option) => ({ value: option.value, label: option.label }))}
      />
      <Select
        name="category-color"
        label="Category Icon Color"
        value={ticketRowSettings.categoryColor.value}
        choices={CATEGORY_COLORS.map((option): SelectChoice => ({ value: option.value, label: option.label }))}
      />
      <wa-input name="tags" label="Tags (Comma Separated)" value={ticketRowSettings.tags.value} />
      <wa-input name="agent" label="Active Agent" value={ticketRowSettings.agentName.value} />
      <Checkbox name="up-next" checked={ticketRowSettings.upNext.value} label="Up Next" />
      <Checkbox name="blocked" checked={ticketRowSettings.blocked.value} label="Blocked" />
      <Checkbox name="needs-review" checked={ticketRowSettings.needsReview.value} label="Needs review" />
      <Checkbox name="feedback-needed" checked={ticketRowSettings.feedbackNeeded.value} label="Feedback needed" />
      <Checkbox name="selected" checked={ticketRowSettings.selected.value} label="Selected" />
      <Checkbox name="busy" checked={ticketRowSettings.busy.value} label="AI working" />
      <Checkbox
        name="show-source-icon"
        checked={ticketRowSettings.showSourceIcon.value}
        label="Multiple ticket sources"
      />
      <Select
        name="claim-eta"
        label="Claim ETA (Shown While AI Working)"
        value={ticketRowSettings.claimEta.value}
        choices={[
          { value: 'none', label: 'No Estimate' },
          { value: 'estimate', label: 'On Track (~45m Left)' },
          { value: 'overrun', label: 'Past Its Estimate (Soon)' },
        ]}
      />
      <Select
        name="phase-estimate"
        label="Testing Phase Estimate (Shown in Initial or Final Testing)"
        value={ticketRowSettings.phaseEstimate.value}
        choices={[
          { value: 'none', label: 'No Estimate' },
          { value: 'estimate', label: 'On Track (~6m Left)' },
          { value: 'overrun', label: 'Past Typical Duration (Soon)' },
        ]}
      />
      <Select
        name="confidence"
        label="Completion Confidence (Shown When Completed or Verified)"
        value={ticketRowSettings.confidence.value}
        choices={TICKET_ROW_CONFIDENCE_CHOICES}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTicketRowDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  // The danger row's select action toggles the shared Selected state, as the app opens its recovery.
  demoListeners.add(
    delegate(
      root,
      'click',
      `[data-ticket-row-demo-danger] ${NAVIGATION_AND_TABS_ACTIONS.selectCorruptTicket.selector}`,
      () => {
        ticketRowSettings.selected.value = !ticketRowSettings.selected.value;
        ticketRowSettings.event.value = ticketRowSettings.selected.value
          ? 'Recovery opened for HS2-BROKEN'
          : 'Recovery closed for HS2-BROKEN';
        const selected = root.querySelector<FormControl>('[data-settings="ticket-list-row"] [name="selected"]');
        if (selected) selected.checked = ticketRowSettings.selected.value;
      },
    ),
  );
  demoListeners.add(
    delegate(root, 'input', '[data-settings="ticket-list-row"] wa-input', (_event, target) => {
      const control = target as FormControl;
      if (control.getAttribute('name') === 'title') ticketRowSettings.title.value = control.value;
      if (control.getAttribute('name') === 'category') ticketRowSettings.category.value = control.value;
      if (control.getAttribute('name') === 'tags') ticketRowSettings.tags.value = control.value;
      if (control.getAttribute('name') === 'agent') ticketRowSettings.agentName.value = control.value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="ticket-list-row"] [name]', (_event, target) => {
      const control = target as FormControl;
      switch (control.getAttribute('name')) {
        case 'status':
          ticketRowSettings.status.value = control.value as typeof ticketRowSettings.status.value;
          break;
        case 'started-phase':
          ticketRowSettings.startedPhase.value = control.value as typeof ticketRowSettings.startedPhase.value;
          break;
        case 'priority':
          ticketRowSettings.priority.value = control.value as typeof ticketRowSettings.priority.value;
          break;
        case 'category-icon':
          ticketRowSettings.categoryIcon.value = control.value;
          break;
        case 'category-color':
          ticketRowSettings.categoryColor.value = control.value;
          break;
        case 'up-next':
          ticketRowSettings.upNext.value = control.checked;
          break;
        case 'blocked':
          ticketRowSettings.blocked.value = control.checked;
          break;
        case 'needs-review':
          ticketRowSettings.needsReview.value = control.checked;
          break;
        case 'feedback-needed':
          ticketRowSettings.feedbackNeeded.value = control.checked;
          break;
        case 'selected':
          ticketRowSettings.selected.value = control.checked;
          break;
        case 'busy':
          ticketRowSettings.busy.value = control.checked;
          break;
        case 'show-source-icon':
          ticketRowSettings.showSourceIcon.value = control.checked;
          break;
        case 'claim-eta':
          ticketRowSettings.claimEta.value = control.value as typeof ticketRowSettings.claimEta.value;
          break;
        case 'phase-estimate':
          ticketRowSettings.phaseEstimate.value = control.value as typeof ticketRowSettings.phaseEstimate.value;
          break;
        case 'confidence':
          ticketRowSettings.confidence.value = control.value;
          break;
        case null:
          break;
      }
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-row': {
      catalogExamples: true,
      Demo: TicketRowDemo,
      Settings: TicketRowSettings,
      reset: resetTicketRowDemo,
    },
  },
  wire: wireTicketRowDemo,
};
