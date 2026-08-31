import '@awesome.me/webawesome/dist/components/button/button.js';
import './note-composer.css';

export function NoteComposer({ value = '' }: { value?: string }) {
  return <form class="note-composer" data-component="note-composer" data-action="create-note-form">
    <textarea name="new-note-body" aria-label="New note" placeholder="Write a note…" autofocus>{value}</textarea>
    <footer><wa-button appearance="plain" type="button" data-action="cancel-new-note">Cancel</wa-button><wa-button appearance="accent" type="submit" disabled={!value.trim()}>Add note</wa-button></footer>
  </form>;
}
