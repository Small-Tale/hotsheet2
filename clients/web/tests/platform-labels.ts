/**
 * Host "reveal file" labels as the client derives them from the browser's platform. Playwright
 * runs its browsers on the test host, so the host platform predicts them: Finder on macOS, File
 * Explorer on Windows, and a generic file-manager label on Linux CI (HS2-6H4M85).
 */
const mac = process.platform === 'darwin',
  windows = process.platform === 'win32';

/** The attachment menu's reveal action (`features/gallery.tsx`). */
export const attachmentRevealLabel = mac
  ? 'Show in Finder'
  : windows
    ? 'Show in File Explorer'
    : 'Show in file manager';

/** The corrupt-ticket recovery reveal action (`components/corrupt-ticket-row.tsx`). */
export const ticketFileRevealLabel = mac
  ? 'Reveal in Finder'
  : windows
    ? 'Show in File Explorer'
    : 'Show file location';
