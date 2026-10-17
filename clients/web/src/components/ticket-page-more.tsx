import { Button } from '@kerfjs/ui/button';
import { Row } from '@kerfjs/ui/row';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';

export interface TicketPageMoreProps {
  /** The next page is loading: the button is disabled and says so. */
  loading?: boolean;
}

/**
 * The "Load more tickets" continuation below a paged ticket list or board column (HS2-WP69TD).
 * Both the list workspace and TicketBoardColumn compose this one control instead of sharing a
 * global class. A centered Kerf Row places a filled-outlined pill `wa-button` whose own `loading`
 * state replaces the former hand-styled button and its stylesheet (HS2-TCM7WC).
 */
export function TicketPageMore({ loading = false }: TicketPageMoreProps) {
  return (
    <Row hAlign="center" controlInsets="tb">
      <Button
        type="button"
        className={'ticket-page-more'}
        emphasis="secondary"
        shape="pill"
        loading={loading}
        disabled={loading}
        action={INSPECTOR_AND_EDITOR_ACTIONS.loadNextTicketPage.value}
      >
        {loading ? 'Loading…' : 'Load more tickets'}
      </Button>
    </Row>
  );
}
