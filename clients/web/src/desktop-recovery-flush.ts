import { DESKTOP_RECOVERY_FLUSH_EVENT, type DesktopRecoveryFlush } from './desktop-close-contract';

/** DOM dispatch reports thrown listener errors globally; preserve them as a failed close instead. */
export function listenForDesktopRecovery(
  host: Pick<Window, 'addEventListener'>,
  capture: () => void,
  options?: AddEventListenerOptions,
) {
  host.addEventListener(
    DESKTOP_RECOVERY_FLUSH_EVENT,
    (event: Event) => {
      try {
        capture();
      } catch {
        (event as CustomEvent<DesktopRecoveryFlush>).detail.failed = true;
      }
    },
    options,
  );
}
