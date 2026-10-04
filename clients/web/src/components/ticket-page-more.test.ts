import { describe, expect, it } from 'vitest';

import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import { PROJECT_LIFECYCLE_ACTIONS } from '../interaction-attrs/project-lifecycle';
import { ProviderSetupBackButton } from './provider-setup-form';
import { ChangeEvidenceDialog } from './repository-status-popover';
import { DiffToolMissingNotice } from './ticket-code-review';
import { TicketPageMore } from './ticket-page-more';

// Components compose the owner of a shared presentation rather than rendering its class (HS2-WP69TD).
describe('extracted shared presentations', () => {
  it('renders the idle and loading ticket page continuation', () => {
    const idle = String(TicketPageMore({}));
    expect(idle).toContain('class="ticket-page-more"');
    expect(idle).toContain('data-action="load-next-ticket-page"');
    expect(idle).toContain('Load more tickets');
    expect(idle).not.toContain('disabled');
    const loading = String(TicketPageMore({ loading: true }));
    expect(loading).toContain('disabled');
    expect(loading).toContain('Loading…');
  });

  it('shows the code review diff-tool notice inside the change evidence dialog only without a diff tool', () => {
    const notice = String(DiffToolMissingNotice());
    expect(notice).toContain('class="ticket-code-review__notice"');
    expect(notice).toContain('role="status"');
    const review = { truncated: false, ranges: [], commits: [], files: [] };
    const without = String(
      ChangeEvidenceDialog({ review: { ...review, difftool: undefined }, view: 'docs', embedded: true }),
    );
    expect(without).toContain(notice);
    const withTool = String(
      ChangeEvidenceDialog({ review: { ...review, difftool: 'Meld' }, view: 'docs', embedded: true }),
    );
    expect(withTool).not.toContain('ticket-code-review__notice');
  });

  it('renders the ticket-source back control with each screen own back action', () => {
    for (const action of [COMMANDS_AND_AI_ACTIONS.backProviderKind, PROJECT_LIFECYCLE_ACTIONS.backTicketStoreRemote]) {
      const markup = String(ProviderSetupBackButton({ action }));
      // It composes the shared FlowBackButton, the one back affordance across setup dialogs (HS2-WJ4JDW).
      expect(markup).toContain('class="flow-back-button"');
      expect(markup).toContain('type="button"');
      expect(markup).toContain(`data-action="${action.value}"`);
      expect(markup).toContain('Ticket source types');
      expect(markup).toContain('data-lucide="chevron-left"');
    }
  });
});
