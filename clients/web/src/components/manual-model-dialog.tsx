import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './manual-model-dialog.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { Text } from '@kerfjs/ui/text';

export interface ManualModelDialogState {
  target: 'settings' | 'drive' | 'conversation' | 'command';
  providerName: string;
  value: string;
  commandId?: string;
}

export function ManualModelDialog({ state }: { state?: ManualModelDialogState }) {
  if (!state) return <></>;
  return (
    <wa-dialog
      class="manual-model-dialog"
      data-component="manual-model-dialog"
      label="Other model"
      aria-label="Other model"
      open={Boolean(state) || undefined}
    >
      <form data-action="submit-manual-model">
        <List className="manual-model-dialog__form" gap="l">
          <Text tone="quiet">Enter the exact model identifier accepted by {state.providerName}.</Text>
          <wa-input name="manual-model" label="Model identifier" value={state.value} required autofocus></wa-input>
          <footer>
            <Row vAlign="middle" gap="xs">
              <Spacer flex />
              <wa-button appearance="plain" type="button" data-action="cancel-manual-model">
                Cancel
              </wa-button>
              <wa-button appearance="accent" type="submit">
                Use model
              </wa-button>
            </Row>
          </footer>
        </List>
      </form>
    </wa-dialog>
  );
}
