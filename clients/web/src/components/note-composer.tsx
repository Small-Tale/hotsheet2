import '@awesome.me/webawesome/dist/components/button/button.js';
import './note-composer.css';

import { INSPECTOR_AND_EDITOR_ACTIONS } from '../interaction-attrs/inspector-and-editor';

export function NoteComposer({ value = '' }: { value?: string }) {
  return (
    <form class="note-composer" data-component="note-composer" {...INSPECTOR_AND_EDITOR_ACTIONS.createNoteForm.attrs}>
      <textarea name="new-note-body" aria-label="New note" placeholder="Write a note…" autofocus>
        {value}
      </textarea>
      <footer>
        <wa-button appearance="plain" type="button" {...INSPECTOR_AND_EDITOR_ACTIONS.cancelNewNote.attrs}>
          Cancel
        </wa-button>
        <wa-button appearance="accent" type="submit" disabled={!value.trim()}>
          Add note
        </wa-button>
      </footer>
    </form>
  );
}
