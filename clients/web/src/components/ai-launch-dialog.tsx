import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Spacer } from '@kerfjs/ui/spacer';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { TextField } from '@kerfjs/ui/text-field';

import type { AiLaunchDialogState } from '../ai-launch-dialog';
import { INPUT_DIALOG_ACTIONS as actions, INPUT_DIALOG_TARGETS as targets } from '../interaction-attrs/input-dialogs';

export function AiLaunchDialog({ state }: { state?: AiLaunchDialogState }) {
  if (!state) return null;
  return (
    <DialogSurface
      label={state.kind === 'ai-shell' ? 'New AI Terminal' : 'New AI Chat'}
      className="ai-launch-dialog"
      preferredWidth={remify(448)}
      viewportGutter={remify(16)}
      rootAttributes={{
        'data-key': 'ai-launch-dialog',
        'data-ai-launch-dialog': 'true',
        'data-controlled-open': 'true',
      }}
    >
      <form {...actions.submitAiLaunch.attrs}>
        <List gap="l">
          <TextField {...targets.provider.attrs} label="AI Provider" value={state.provider} autofocus />
          <TextField {...targets.model.attrs} label="Model" value={state.model} placeholder="Provider default" />
          <Select
            {...targets.effort.attrs}
            label="Effort Level"
            value={state.effort || '__provider-default__'}
            disabled={!state.effortLevels.length}
            choices={[
              { value: '__provider-default__', label: 'Provider Default' },
              ...state.effortLevels.map((value) => ({ value, label: value })),
            ]}
          />
          {state.message ? (
            <StateBanner tone="danger" title="Launch Could Not Be Prepared" detail={state.message} />
          ) : null}
          <Row vAlign="middle" gap="xs">
            <Button emphasis="plain" type="button" action={actions.cancelAiLaunch.value}>
              Cancel
            </Button>
            <Spacer flex />
            <Button emphasis="primary" type="submit">
              {state.kind === 'ai-shell' ? 'Create Terminal' : 'Create Chat'}
            </Button>
          </Row>
        </List>
      </form>
    </DialogSurface>
  );
}
