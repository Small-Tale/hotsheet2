/** HS2-E0H7S0: capture local recovery without tearing down pagehide-owned streams. */
export const DESKTOP_RECOVERY_FLUSH_EVENT = 'hotsheet-desktop-flush';
export const DESKTOP_CLOSE_COMMAND = 'desktop_flush_complete';

export interface DesktopRecoveryFlush {
  failed: boolean;
}

export interface DesktopCloseHost {
  __HOTSHEET_DESKTOP_CLOSE__?: (request: string) => void;
  __HOTSHEET_DESKTOP_CLOSE_CANCEL__?: (request: string) => void;
  __TAURI_INTERNALS__?: {
    invoke: (command: string, args: { request: string; success: boolean }) => Promise<unknown>;
  };
  dispatchEvent(event: Event): boolean;
  alert(message: string): void;
  document: Pick<Document, 'documentElement'>;
}
