import { attr } from 'kerfjs';
import { action } from 'kerfjs/actions';

export const INPUT_DIALOG_ACTIONS = {
  submitCommandGroup: action('submit-command-group'),
  cancelCommandGroup: action('cancel-command-group'),
  submitAiLaunch: action('submit-ai-launch'),
  cancelAiLaunch: action('cancel-ai-launch'),
} as const;

export const INPUT_DIALOG_TARGETS = {
  group: attr('data-command-group-dialog', 'true'),
  ai: attr('data-ai-launch-dialog', 'true'),
  groupName: attr('name', 'command-group-name'),
  provider: attr('name', 'launch-provider'),
  model: attr('name', 'launch-model'),
  effort: attr('name', 'launch-effort'),
  filename: attr('name', 'attachment-filename'),
} as const;
