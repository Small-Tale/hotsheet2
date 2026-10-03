import './app-error.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { X } from 'lucide';

import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';

export function AppError({ message }: { message: string }) {
  return (
    <div class="app-error" data-component="app-error" role="alert">
      <span>{message}</span>
      <button
        type="button"
        {...NOTIFICATIONS_AND_LINKS_ACTIONS.dismissAppError.attrs}
        aria-label="Dismiss error"
        title="Dismiss error"
      >
        <LucideIcon size="s" icon={X} name="x" />
      </button>
    </div>
  );
}
