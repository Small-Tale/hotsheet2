import './ticket-page-more.css';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';

export interface TicketPageMoreProps {
  /** The next page is loading: the button is disabled and says so. */
  loading?: boolean;
}

/**
 * The "Load more tickets" continuation below a paged ticket list or board column (HS2-WP69TD).
 * Both the list workspace and TicketBoardColumn compose this one button instead of sharing a
 * global class.
 */
export function TicketPageMore({ loading = false }: TicketPageMoreProps) {
  return (
    <button
      type="button"
      class="ticket-page-more"
      {...INSPECTOR_AND_EDITOR_ACTIONS.loadNextTicketPage.attrs}
      disabled={loading}
    >
      {loading ? 'Loading…' : 'Load more tickets'}
    </button>
  );
}
