import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';

import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';

export interface ManualModelDialogState {
  target: 'settings' | 'drive' | 'conversation' | 'command';
  providerName: string;
  value: string;
  commandId?: string;
  /** The provider whose default model the Settings "Other…" choice sets (HS2-EK24KF). */
  providerId?: string;
}

export function ManualModelDialog({ state }: { state?: ManualModelDialogState }) {
  if (!state) return <></>;
  return (
    <div data-key="manual-model-dialog">
      <DialogSurface
        label="Other model"
        preferredWidth={remify(448)}
        viewportGutter={remify(16)}
        className="manual-model-dialog"
        rootAttributes={{
          'data-key': 'manual-model-dialog',
          ...COMMANDS_AND_AI_TARGETS.manualModelDialog.attrs,
          'data-controlled-open': 'true',
        }}
      >
        <form {...COMMANDS_AND_AI_ACTIONS.submitManualModel.attrs}>
          <List className="manual-model-dialog__form" gap="l">
            <Text tone="quiet">Enter the exact model identifier accepted by {state.providerName}.</Text>
            <wa-input name="manual-model" label="Model Identifier" value={state.value} required autofocus />
            <footer>
              <Row vAlign="middle" gap="xs">
                <Spacer flex />
                <wa-button appearance="plain" type="button" {...COMMANDS_AND_AI_ACTIONS.cancelManualModel.attrs}>
                  Cancel
                </wa-button>
                <wa-button appearance="accent" type="submit">
                  Use Model
                </wa-button>
              </Row>
            </footer>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
