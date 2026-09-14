import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './trash-settings.css';

export function TrashSettings({ days, message = '' }: { days: number; message?: string }) {
  return <form class="trash-settings" data-component="trash-settings" data-action="save-trash-settings">
    <header>
      <h2>Trash retention</h2>
      <p>Deleted tickets remain recoverable until the automatic cleanup removes them. The default is 30 days. Git history keeps every purged ticket file.</p>
    </header>
    <wa-input name="trash-cleanup-days" type="number" label="Keep deleted tickets for (days)" value={String(days)} required></wa-input>
    <footer>
      <wa-button type="submit" variant="brand">Save retention</wa-button>
      <span role="status">{message}</span>
    </footer>
  </form>;
}
