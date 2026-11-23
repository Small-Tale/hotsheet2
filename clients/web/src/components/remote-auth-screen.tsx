import './remote-auth-screen.css';

import { mount, signal } from 'kerfjs';

import { setBrowserSessionGrants, stampedBrowserAuthMode } from '../client-origin';
import { browserSelfDescription, defaultDeviceName } from '../device-name';
import { remoteSessionEndCode } from '../features/browser-grants';

type Pairing = { id: string; server_id: string; challenge: string; expires_at: number };

/**
 * What this browser needs (HS2-4B0KSD): a first pairing (`pair`), a renewal of an approval that
 * expired (`expired`, then `renewal-requested`), a new pairing after the host revoked it (`revoked`), or
 * nothing but a host that answers again (`unreachable`). Each has its own copy, so a restarting host is
 * never mistaken for a lost approval.
 */
export type RemoteAuthMode = 'pair' | 'expired' | 'renewal-requested' | 'revoked' | 'unreachable';

const COPY: Record<RemoteAuthMode, { title: string; detail: string }> = {
  pair: {
    title: 'Pair this browser with Hot Sheet',
    detail: 'The owner approves this browser on the host Mac. Approval gives this browser a revocable session.',
  },
  expired: {
    title: 'This browser’s approval expired',
    detail:
      'An approval lasts 90 days, or 30 days without use. Renew asks the host to approve this browser again; it keeps its name, access and settings.',
  },
  'renewal-requested': {
    title: 'Renewal requested',
    detail:
      'Approve it on the host in Settings → Devices, or on the host Mac run hotsheet-cli remote sessions renew with this device’s id, then choose Check Again.',
  },
  revoked: {
    title: 'This browser’s access was revoked',
    detail: 'The host removed this browser’s approval. Request access to pair it again.',
  },
  unreachable: {
    title: 'Hot Sheet is not answering',
    detail:
      'The host did not answer. It may be restarting. This browser’s approval is unchanged; Hot Sheet checks again when you return to this page, or choose Try Again.',
  },
};

export function RemoteAuthScreen({
  label,
  pairing,
  message,
  busy,
  mode = 'pair',
}: {
  label: string;
  pairing: Pairing | undefined;
  message: string;
  busy: boolean;
  mode?: RemoteAuthMode;
}) {
  const pairs = mode === 'pair' || mode === 'revoked',
    copy = pairing ? COPY.pair : COPY[mode];
  return (
    <main class="remote-auth-screen" data-component="remote-auth-screen" data-remote-auth-mode={mode}>
      <section class="remote-auth-screen__card" aria-labelledby="remote-auth-title">
        <p class="remote-auth-screen__eyebrow">Remote access</p>
        <h1 id="remote-auth-title">{copy.title}</h1>
        <p>{copy.detail}</p>
        {!pairs && !pairing ? null : !pairing ? (
          <label class="remote-auth-screen__field">
            Device name
            <input name="device-label" autocomplete="off" maxLength={80} value={label} />
          </label>
        ) : (
          <div class="remote-auth-screen__request" role="status">
            <p>
              <strong>Request ID</strong> <code>{pairing.id}</code>
            </p>
            <p>
              <strong>Server ID</strong> <code>{pairing.server_id}</code>
            </p>
            <p>
              Approve the request from Settings → Devices on the host or on an administrator device, or on the host Mac
              run <code>hotsheet-cli remote sessions status</code>, verify both IDs and the device name, then approve
              it. This request expires in 15 minutes.
            </p>
          </div>
        )}
        {message && (
          <p class="remote-auth-screen__message" role="alert">
            {message}
          </p>
        )}
        <div class="remote-auth-screen__actions">
          {mode === 'expired' && !pairing ? (
            <>
              <button type="button" data-remote-action="renew" disabled={busy}>
                Renew
              </button>
              <button type="button" data-remote-action="pair-new" disabled={busy}>
                Pair as a New Device
              </button>
            </>
          ) : mode === 'renewal-requested' && !pairing ? (
            <button type="button" data-remote-action="check" disabled={busy}>
              Check Again
            </button>
          ) : mode === 'unreachable' && !pairing ? (
            <button type="button" data-remote-action="check" disabled={busy}>
              Try Again
            </button>
          ) : !pairing ? (
            <button type="button" data-remote-action="request" disabled={busy}>
              Request Access
            </button>
          ) : (
            <>
              <button type="button" data-remote-action="continue" disabled={busy}>
                Continue After Approval
              </button>
              <button type="button" data-remote-action="restart" disabled={busy}>
                Start Again
              </button>
            </>
          )}
        </div>
      </section>
    </main>
  );
}

/** HS2-7DBJMS: the challenge stays in this page's memory only. A reload starts a new request. */
export async function ensureRemoteBrowserSession(): Promise<boolean> {
  // The serving host stamps this into the document. A separate mode request can be intercepted by
  // project API fixtures and would leave an otherwise local application blank (HS2-7DBJMS).
  const mode = stampedBrowserAuthMode();
  if (!mode) throw new Error('Could not determine browser authentication mode.');
  if (mode === 'local') return false;
  const first = await checkRemoteSession();
  if (first === 'ok') return true;
  const root = document.querySelector<HTMLElement>('#app');
  if (!root) throw new Error('Hot Sheet application root is missing.');

  const label = signal(defaultDeviceName(browserSelfDescription()));
  const pairing = signal<Pairing | undefined>(undefined);
  const message = signal('');
  const busy = signal(false);
  const screen = signal<RemoteAuthMode>(first);
  await new Promise<void>((resolve) => {
    const unmount = mount(root, () => (
      <RemoteAuthScreen
        label={label.value}
        pairing={pairing.value}
        message={message.value}
        busy={busy.value}
        mode={screen.value}
      />
    ));
    const finish = () => {
      root.removeEventListener('click', clickEvent);
      root.removeEventListener('input', input);
      window.removeEventListener('focus', recheck);
      window.removeEventListener('online', recheck);
      unmount();
      resolve();
    };
    /** Check the session again: a renewed or reachable session opens the application. */
    const check = async () => {
      const next = await checkRemoteSession();
      if (next === 'ok') {
        finish();
        return;
      }
      // A requested renewal stays requested until the host renews it or it is refused.
      if (!(screen.value === 'renewal-requested' && next === 'expired')) screen.value = next;
      message.value = next === 'expired' && screen.value === 'renewal-requested' ? 'Not renewed yet.' : '';
    };
    // Returning to the page or regaining the network checks once; nothing polls.
    const recheck = () => {
      if (!busy.value && !pairing.value && screen.value !== 'pair' && screen.value !== 'revoked') void check();
    };
    window.addEventListener('focus', recheck);
    window.addEventListener('online', recheck);
    const input = (event: Event) => {
      const target = event.target;
      if (target instanceof HTMLInputElement && target.name === 'device-label') label.value = target.value;
    };
    const click = async (event: Event) => {
      const target = event.target;
      if (!(target instanceof Element)) return;
      const action = target.closest<HTMLElement>('[data-remote-action]')?.dataset.remoteAction;
      if (!action || busy.value) return;
      if (action === 'restart') {
        pairing.value = undefined;
        message.value = '';
        return;
      }
      if (action === 'pair-new') {
        screen.value = 'pair';
        message.value = '';
        return;
      }
      busy.value = true;
      message.value = '';
      try {
        if (action === 'check') {
          await check();
        } else if (action === 'renew') {
          const response = await fetch('/__hotsheet/remote-auth/renewal', {
            method: 'POST',
            credentials: 'same-origin',
          });
          if (response.ok) screen.value = 'renewal-requested';
          else if (response.status === 401) screen.value = await failureMode(response);
          else throw new Error('The host did not answer. Try again.');
        } else if (action === 'request') {
          const response = await fetch('/__hotsheet/remote-auth/pair', {
            method: 'POST',
            credentials: 'same-origin',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ label: label.value.trim() }),
          });
          if (!response.ok)
            throw new Error(
              response.status === 429 ? 'Too many requests. Wait a minute and try again.' : 'Could not request access.',
            );
          pairing.value = (await response.json()) as Pairing;
        } else if (action === 'continue' && pairing.value) {
          const response = await fetch('/__hotsheet/remote-auth/redeem', {
            method: 'POST',
            credentials: 'same-origin',
            headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ id: pairing.value.id, challenge: pairing.value.challenge }),
          });
          if (!response.ok)
            throw new Error(
              response.status === 409
                ? 'Waiting for host approval.'
                : response.status === 410
                  ? 'Request expired. Start again.'
                  : 'Access was not granted.',
            );
          setBrowserSessionGrants(await response.json().catch(() => undefined));
          finish();
        }
      } catch (error) {
        message.value = error instanceof Error ? error.message : 'Could not pair this browser.';
      } finally {
        busy.value = false;
      }
    };
    const clickEvent = (event: Event) => {
      void click(event);
    };
    root.addEventListener('click', clickEvent);
    root.addEventListener('input', input);
  });
  return true;
}

/** Read this browser's session once (HS2-4B0KSD): usable, or which screen explains why not. */
async function checkRemoteSession(): Promise<'ok' | RemoteAuthMode> {
  let response: Response;
  try {
    response = await fetch('/__hotsheet/remote-auth/me', { credentials: 'same-origin' });
  } catch {
    return 'unreachable';
  }
  if (response.ok) {
    // HS2-85W6BE: the session's grants, not the URL, decide host-only actions and device management.
    setBrowserSessionGrants(await response.json().catch(() => undefined));
    return 'ok';
  }
  if (response.status === 401) return failureMode(response);
  return 'unreachable';
}

/** The screen for a 401 by the bridge's code: expired, revoked, or a first pairing. */
async function failureMode(response: Response): Promise<RemoteAuthMode> {
  const code = remoteSessionEndCode(await response.json().catch(() => undefined));
  if (code === 'session_expired') return 'expired';
  if (code === 'session_revoked') return 'revoked';
  return 'pair';
}
