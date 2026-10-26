import {
  DESKTOP_CLOSE_COMMAND,
  DESKTOP_RECOVERY_FLUSH_EVENT,
  type DesktopCloseHost,
  type DesktopRecoveryFlush,
} from './desktop-close-contract';

const failureMessage = 'Local changes could not be saved. Hot Sheet will stay open. Try closing again.';

/** The native shell owns the deadline and accepts only its current request's successful ACK. */
export function installDesktopClose(
  persistence: { flushForNativeClose(): Promise<void> },
  host: DesktopCloseHost = window,
) {
  let current: string | undefined,
    wasInert = false,
    lastRequest = 0n;
  const release = (request: string, failed: boolean) => {
    if (current !== request) return;
    current = undefined;
    host.document.documentElement.inert = wasInert;
    if (failed) host.alert(failureMessage);
  };
  host.__HOTSHEET_DESKTOP_CLOSE_CANCEL__ = (request) => {
    if (!/^[1-9][0-9]{0,19}$/.test(request)) return;
    if (BigInt(request) > lastRequest) {
      // Native can time out a request made before this listener existed. Consume
      // that generation so its delayed delivery cannot silently restart close.
      if (current === undefined) wasInert = host.document.documentElement.inert;
      lastRequest = BigInt(request);
      current = request;
    }
    release(request, true);
  };
  host.__HOTSHEET_DESKTOP_CLOSE__ = (request) => {
    if (!/^[1-9][0-9]{0,19}$/.test(request) || BigInt(request) <= lastRequest) return;
    lastRequest = BigInt(request);
    if (current === undefined) wasInert = host.document.documentElement.inert;
    current = request;
    host.document.documentElement.inert = true;
    void (async () => {
      let success = false;
      try {
        const capture = { failed: false };
        host.dispatchEvent(new CustomEvent<DesktopRecoveryFlush>(DESKTOP_RECOVERY_FLUSH_EVENT, { detail: capture }));
        if (capture.failed) throw new Error('Could not capture local recovery.');
        await persistence.flushForNativeClose();
        success = true;
      } catch {
        // Retain the recovery queue; neither content nor errors cross the native boundary.
      }
      if (current !== request) return;
      try {
        if (!host.__TAURI_INTERNALS__) throw new Error('Native acknowledgement unavailable.');
        await host.__TAURI_INTERNALS__.invoke(DESKTOP_CLOSE_COMMAND, { request, success });
      } catch {
        success = false;
      }
      if (!success) release(request, true);
    })();
  };
}
