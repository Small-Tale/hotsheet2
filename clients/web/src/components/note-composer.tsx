import './native-textarea.css';
import './note-composer.css';

import { Button } from '@kerfjs/ui/button';
import { ContentItem } from '@kerfjs/ui/content-item';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';

export function NoteComposer({ value = '' }: { value?: string }) {
  return (
    <ContentItem appearance="surface" frame="framed">
      <form class="note-composer" data-component="note-composer" {...INSPECTOR_AND_EDITOR_ACTIONS.createNoteForm.attrs}>
        <List gap="xs">
          <textarea
            class="native-textarea"
            name="new-note-body"
            aria-label="New note"
            placeholder="Write a note…"
            autofocus
          >
            {value}
          </textarea>
          <Row hAlign="right" vAlign="middle" gap="xs">
            <Button emphasis="plain" type="button" action={INSPECTOR_AND_EDITOR_ACTIONS.cancelNewNote.value}>
              Cancel
            </Button>
            <Button emphasis="primary" type="submit" disabled={!value.trim()}>
              Add Note
            </Button>
          </Row>
        </List>
      </form>
    </ContentItem>
  );
}
