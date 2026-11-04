import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import type { ConfirmationRequest } from '../confirmation';
import { CONFIRMATION_ACTIONS, CONFIRMATION_TARGETS } from '../interaction-attrs/confirmation';

const MESSAGE_ID = 'confirmation-dialog-message';

/**
 * The shared in-app confirmation (HS2-6S1346): a Kerf alert `DialogSurface` asking one question
 * before an action undo cannot cover. Cancel leads and the confirming action trails, per Kerf's
 * "Actions, feedback, and dialogs". `createConfirmation` owns the request; `wireConfirmation`
 * answers it from the buttons, Escape, or the close button.
 */
export function ConfirmationDialog({ request }: { request?: ConfirmationRequest }) {
  if (!request) return null;
  const key = `confirmation-dialog-${String(request.id)}`;
  return (
    <div data-key={key}>
      <DialogSurface
        label={request.title}
        alert
        describedBy={request.message ? MESSAGE_ID : undefined}
        size="small"
        bodyInset="none"
        viewportGutter={remify(16)}
        rootAttributes={{
          'data-key': key,
          ...CONFIRMATION_TARGETS.dialog.attrs,
          'data-confirmation-id': String(request.id),
          'data-controlled-open': 'true',
        }}
        footer={
          // The footer row fills the footer so the Spacer separates Cancel from the trailing action.
          <Row vAlign="middle" gap="xs" flex>
            <Button
              type="button"
              emphasis="secondary"
              action={CONFIRMATION_ACTIONS.cancel.value}
              rootAttributes={{ 'data-confirmation-id': String(request.id) }}
            >
              {request.cancelLabel ?? 'Cancel'}
            </Button>
            <Spacer flex />
            <Button
              type="button"
              tone={request.tone ?? 'brand'}
              action={CONFIRMATION_ACTIONS.confirm.value}
              rootAttributes={{ 'data-confirmation-id': String(request.id) }}
            >
              {request.confirmLabel}
            </Button>
          </Row>
        }
      >
        {/* ListInsetText puts the message on the title's 17px text edge. */}
        {request.message ? (
          <ListInsetText>
            <Text flush id={MESSAGE_ID}>
              {request.message}
            </Text>
          </ListInsetText>
        ) : null}
      </DialogSurface>
    </div>
  );
}
