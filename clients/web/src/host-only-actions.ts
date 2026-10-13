import { isRemoteClient } from './client-origin';

/** Actions that launch a process or native dialog on the machine running the Hot Sheet server
 * (HS2-F78B1S). On a remote or mobile client they would act on a screen nobody is looking at, so the
 * client refuses them with an explanation instead (inventory: HS2-E0HC6J). */
export type HostOnlyAction =
  'diff-tool' | 'open-file' | 'reveal-file' | 'choose-folder' | 'conversation-export' | 'conversation-open';

const ACTION_LABELS: Record<HostOnlyAction, string> = {
  'diff-tool': 'The diff tool opens',
  'open-file': 'Opening a file happens',
  'reveal-file': 'Revealing a file happens',
  'choose-folder': 'The folder chooser opens',
  'conversation-export': 'Choosing an export folder happens',
  'conversation-open': 'Opening a saved conversation happens',
};

export class HostOnlyActionError extends Error {
  constructor(readonly action: HostOnlyAction) {
    super(hostOnlyActionMessage(action));
    this.name = 'HostOnlyActionError';
  }
}

export function hostOnlyActionMessage(action: HostOnlyAction): string {
  return `${ACTION_LABELS[action]} on the computer running Hot Sheet, so it isn't available from this device.`;
}

/** Whether host-only controls can act from this client (HS2-TYS5SS). Components take it as their
 * `hostActionsAvailable` prop default, so remote clients render those controls disabled with the
 * {@link hostOnlyActionMessage} tooltip before anyone clicks. */
export function hostActionsAvailableHere(): boolean {
  return !(typeof window !== 'undefined' && isRemoteClient());
}

/** The disabled-control tooltip for a host-only action, or undefined when the action is available. */
export function hostOnlyDisabledReason(action: HostOnlyAction, available: boolean): string | undefined {
  return available ? undefined : hostOnlyActionMessage(action);
}

/** Throw a {@link HostOnlyActionError} when the client is on a different device than the server.
 * `window`-free contexts (Node tests, workers) count as same-device. */
export function assertHostAction(
  action: HostOnlyAction,
  remote: boolean = typeof window !== 'undefined' && isRemoteClient(),
): void {
  if (remote) throw new HostOnlyActionError(action);
}
