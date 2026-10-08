import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import './ai-feedback-dialog.css';

import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';

import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';

export interface AiFeedbackDialogState {
  ticketId: string;
  target: string;
  rating: 'helpful' | 'not_helpful';
  rater: string;
  explanation: string;
  revising: boolean;
}

export function AiFeedbackDialog({ state }: { state?: AiFeedbackDialogState }) {
  if (!state) return <></>;
  const helpful = state.rating === 'helpful';
  const label = helpful ? 'What should Hot Sheet keep doing?' : 'What should Hot Sheet change or stop doing?';
  return (
    <wa-dialog
      class="ai-feedback-dialog"
      {...COMMANDS_AND_AI_TARGETS.aiFeedbackDialog.attrs}
      label={helpful ? 'Helpful feedback' : 'Not helpful feedback'}
      open
      data-controlled-open="true"
    >
      <form {...COMMANDS_AND_AI_ACTIONS.submitAiFeedback.attrs}>
        <label class="ai-feedback-dialog__label" for="ai-feedback-explanation">
          {label} <span>(optional)</span>
        </label>
        <textarea
          id="ai-feedback-explanation"
          name="ai-feedback-explanation"
          aria-label={label}
          placeholder="Write Markdown feedback…"
          rows={6}
          autofocus
        >
          {state.explanation}
        </textarea>
        <footer>
          <Row vAlign="middle" gap="xs">
            <Spacer flex />
            <wa-button appearance="plain" type="button" {...COMMANDS_AND_AI_ACTIONS.cancelAiFeedback.attrs}>
              Cancel
            </wa-button>
            <wa-button appearance="accent" type="submit">
              {state.revising ? 'Update feedback' : 'Save feedback'}
            </wa-button>
          </Row>
        </footer>
      </form>
    </wa-dialog>
  );
}
