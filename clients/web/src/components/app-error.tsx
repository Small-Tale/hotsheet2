import './app-error.css';

import { StateBanner } from '@kerfjs/ui/state-banner';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';

export function AppError({ message }: { message: string }) {
  return (
    <div class="app-error" data-component="app-error">
      <StateBanner
        title={message}
        tone="danger"
        urgency="alert"
        actionPlacement="below"
        action={
          <button type="button" {...NOTIFICATIONS_AND_LINKS_ACTIONS.dismissAppError.attrs}>
            Dismiss error
          </button>
        }
      />
    </div>
  );
}
