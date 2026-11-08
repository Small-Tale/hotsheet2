import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import { INSPECTOR_AND_EDITOR_ACTIONS, INSPECTOR_AND_EDITOR_TARGETS } from '../interaction-attrs/inspector-and-editor';
import type { NoteKind } from './note-card';

export type NoteMetadataMode = 'kind' | 'summary' | 'confidence';
export interface NoteMetadataDialogState {
  noteId: string;
  ticketId: string;
  projectId: string;
  frameId?: string;
  token: string;
  mode: NoteMetadataMode;
  kind: NoteKind;
  summary?: string;
  confidence?: number;
  hasResponses: boolean;
  submitting?: boolean;
  error?: string;
}

const kinds = [
  { value: 'regular', label: 'Note' },
  { value: 'activity', label: 'Activity' },
  { value: 'status', label: 'Status Update' },
  { value: 'feedback_needed', label: 'Feedback Needed' },
];

export function NoteMetadataDialog({ state }: { state?: NoteMetadataDialogState }) {
  if (!state) return <></>;
  const title =
    state.mode === 'kind'
      ? 'Change note type'
      : state.mode === 'summary'
        ? 'Edit timeline summary'
        : 'Correct confidence';
  return (
    <div data-component="note-metadata-dialog" {...INSPECTOR_AND_EDITOR_TARGETS.noteMetadataDialog.attrs}>
      <DialogSurface
        label={title}
        preferredWidth={remify(480)}
        viewportGutter={remify(16)}
        rootAttributes={{ 'data-controlled-open': 'true' }}
      >
        <form {...INSPECTOR_AND_EDITOR_ACTIONS.saveNoteMetadata.attrs}>
          <List gap="m">
            {state.mode === 'kind' && (
              <>
                <Select name="note-metadata-kind" label="Type" value={state.kind} choices={kinds} />
                {state.kind === 'feedback_needed' && !state.hasResponses && (
                  <Text size="compact" tone="danger">
                    Changing this type closes the unanswered feedback request. Save to confirm.
                  </Text>
                )}
              </>
            )}
            {state.mode === 'summary' && (
              <>
                <TextField
                  name="note-metadata-summary"
                  label="Timeline Summary"
                  value={state.summary ?? ''}
                  maxLength={120}
                  autofocus
                />
                <Text size="compact" tone="quiet">
                  Leave this blank to clear the summary.
                </Text>
              </>
            )}
            {state.mode === 'confidence' && (
              <>
                <TextField
                  name="note-metadata-confidence"
                  label="Confidence"
                  type="number"
                  min={0}
                  max={100}
                  step={1}
                  value={state.confidence === undefined ? '' : String(state.confidence)}
                  autofocus
                />
                <Text size="compact" tone="quiet">
                  Enter 0–100, or leave blank to clear the score.
                </Text>
              </>
            )}
            {state.error ? (
              <Text tone="danger" role="alert">
                {state.error}
              </Text>
            ) : null}
            <Row vAlign="middle" gap="xs">
              <Button
                type="button"
                emphasis="secondary"
                action={INSPECTOR_AND_EDITOR_ACTIONS.cancelNoteMetadata.value}
                disabled={state.submitting}
              >
                Cancel
              </Button>
              <Spacer flex />
              <Button type="submit" tone="brand" disabled={state.submitting}>
                {state.submitting
                  ? 'Saving…'
                  : state.kind === 'feedback_needed' && state.mode === 'kind'
                    ? 'Confirm type change'
                    : 'Save'}
              </Button>
            </Row>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
