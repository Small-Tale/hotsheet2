import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { TextField } from '@kerfjs/ui/text-field';

import type { CommandGroupDialogState } from '../command-group-dialog';
import { INPUT_DIALOG_ACTIONS as actions, INPUT_DIALOG_TARGETS as targets } from '../interaction-attrs/input-dialogs';

export function CommandGroupDialog({ state }: { state?: CommandGroupDialogState }) {
  if (!state) return null;
  return (
    <DialogSurface
      label="New Command Group"
      className="command-group-dialog"
      preferredWidth={remify(448)}
      viewportGutter={remify(16)}
      rootAttributes={{
        'data-key': 'command-group-dialog',
        'data-command-group-dialog': 'true',
        'data-controlled-open': 'true',
      }}
    >
      <form {...actions.submitCommandGroup.attrs}>
        <List gap="l">
          <TextField {...targets.groupName.attrs} label="Group Name" value={state.value} autofocus />
          {state.message ? (
            <StateBanner tone="danger" title="Group Could Not Be Created" detail={state.message} />
          ) : null}
          <Row vAlign="middle" gap="xs">
            <Button emphasis="plain" type="button" action={actions.cancelCommandGroup.value}>
              Cancel
            </Button>
            <Spacer flex />
            <Button emphasis="primary" type="submit">
              Create Group
            </Button>
          </Row>
        </List>
      </form>
    </DialogSurface>
  );
}
