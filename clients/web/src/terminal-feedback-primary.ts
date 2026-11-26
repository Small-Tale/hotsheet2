/**
 * HS2-H3S7YQ: the "Receive Feedback Notifications" command a terminal's menus offer. A person may
 * choose a live AI session that accepts pushed messages as its project's feedback-primary session,
 * or clear that choice so the oldest session receives feedback again. Shared by the drawer tab
 * context menu and the terminal tile More actions menu.
 */

/** The server fields that decide the command (`GET /terminals`). */
export interface FeedbackPrimarySession {
  kind?: 'shell' | 'ai';
  alive: boolean;
  feedback_primary?: boolean;
  feedback_primary_chosen?: boolean;
  feedback_eligible?: boolean;
}

/** What choosing the command does: make this terminal the primary, or forget the choice. */
export type FeedbackPrimaryCommand = 'choose' | 'clear';

export interface FeedbackPrimaryMenuItem {
  command: FeedbackPrimaryCommand;
  label: string;
  /** Quiet secondary text beneath the label. */
  description?: string;
  /** Present when the command is unavailable, explaining why. */
  disabledReason?: string;
}

export const RECEIVE_FEEDBACK_LABEL = 'Receive Feedback Notifications';
export const STOP_FEEDBACK_LABEL = 'Stop Receiving Feedback Notifications';

/** The command one terminal's menu offers, enabled or disabled with its reason. */
export function feedbackPrimaryMenuItem(session: FeedbackPrimarySession): FeedbackPrimaryMenuItem {
  const unavailable = (disabledReason: string): FeedbackPrimaryMenuItem => ({
    command: 'choose',
    label: RECEIVE_FEEDBACK_LABEL,
    disabledReason,
  });
  if (session.kind !== 'ai') return unavailable('A shell terminal cannot receive feedback notifications.');
  if (!session.alive) return unavailable('This AI session has ended.');
  if (session.feedback_primary_chosen === true)
    return {
      command: 'clear',
      label: STOP_FEEDBACK_LABEL,
      description: 'The oldest AI session receives them instead.',
    };
  if (session.feedback_primary === true)
    return unavailable('This session already receives feedback notifications as the oldest AI session.');
  if (session.feedback_eligible !== true)
    return unavailable('This AI session cannot receive pushed messages, so it cannot receive feedback notifications.');
  return { command: 'choose', label: RECEIVE_FEEDBACK_LABEL, description: 'Instead of the oldest AI session.' };
}

/** The toast after a command succeeds. */
export function feedbackPrimaryToast(command: FeedbackPrimaryCommand, title: string): string {
  return command === 'choose'
    ? `${title} now receives feedback notifications.`
    : 'Feedback notifications go to the oldest AI session again.';
}
