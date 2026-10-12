import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/textarea/textarea.js';
import '@kerfjs/ui/surface-scaffold/register';

import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';

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
    <DialogSurface size="medium" viewportGutter={remify(16)}>
      <wa-dialog
        data-key="ai-feedback-dialog"
        class="ai-feedback-dialog"
        {...COMMANDS_AND_AI_TARGETS.aiFeedbackDialog.attrs}
        label={helpful ? 'Helpful feedback' : 'Not helpful feedback'}
        open
        data-controlled-open="true"
      >
        <form {...COMMANDS_AND_AI_ACTIONS.submitAiFeedback.attrs}>
          <List gap="m">
            <wa-textarea
              id="ai-feedback-explanation"
              name="ai-feedback-explanation"
              label={label}
              hint="Optional"
              placeholder="Write Markdown feedback…"
              rows={6}
              resize="vertical"
              value={state.explanation}
              autofocus
            />
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
          </List>
        </form>
      </wa-dialog>
    </DialogSurface>
  );
}
