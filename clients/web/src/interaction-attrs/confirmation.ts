import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

/** The shared confirmation dialog's actions (HS2-6S1346). */
export const CONFIRMATION_ACTIONS = {
  confirm: action('confirm-confirmation'),
  cancel: action('cancel-confirmation'),
} as const;

export const CONFIRMATION_TARGETS = {
  dialog: attr('data-confirmation-dialog', 'true'),
} as const;
