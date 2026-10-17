import '@awesome.me/webawesome/dist/components/textarea/textarea.js';

import { Button } from '@kerfjs/ui/button';
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
    <div data-key="ai-feedback-dialog">
      <DialogSurface
        label={helpful ? 'Helpful feedback' : 'Not helpful feedback'}
        size="medium"
        viewportGutter={remify(16)}
        className="ai-feedback-dialog"
        rootAttributes={{
          'data-key': 'ai-feedback-dialog',
          ...COMMANDS_AND_AI_TARGETS.aiFeedbackDialog.attrs,
          'data-controlled-open': 'true',
        }}
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
                <Button emphasis="plain" type="button" action={COMMANDS_AND_AI_ACTIONS.cancelAiFeedback.value}>
                  Cancel
                </Button>
                <Button emphasis="primary" type="submit">
                  {state.revising ? 'Update feedback' : 'Save feedback'}
                </Button>
              </Row>
            </footer>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
