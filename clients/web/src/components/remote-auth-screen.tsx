import './remote-auth-screen.css';

import { mount, signal } from 'kerfjs';

import { setBrowserSessionGrants, stampedBrowserAuthMode } from '../client-origin';
import { browserSelfDescription, defaultDeviceName } from '../device-name';

type Pairing = { id: string; server_id: string; challenge: string; expires_at: number };

export function RemoteAuthScreen({
  label,
  pairing,
  message,
  busy,
}: {
  label: string;
  pairing: Pairing | undefined;
  message: string;
  busy: boolean;
}) {
  return (
    <main class="remote-auth-screen" data-component="remote-auth-screen">
      <section class="remote-auth-screen__card" aria-labelledby="remote-auth-title">
        <p class="remote-auth-screen__eyebrow">Remote access</p>
        <h1 id="remote-auth-title">Pair this browser with Hot Sheet</h1>
        <p>The owner approves this browser on the host Mac. Approval gives this browser a revocable session.</p>
        {!pairing ? (
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
          {!pairing ? (
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
  const current = await fetch('/__hotsheet/remote-auth/me', { credentials: 'same-origin' });
  if (current.ok) {
    // HS2-85W6BE: the session's grants, not the URL, decide host-only actions and device management.
    setBrowserSessionGrants(await current.json().catch(() => undefined));
    return true;
  }
  const root = document.querySelector<HTMLElement>('#app');
  if (!root) throw new Error('Hot Sheet application root is missing.');

  const label = signal(defaultDeviceName(browserSelfDescription()));
  const pairing = signal<Pairing | undefined>(undefined);
  const message = signal('');
  const busy = signal(false);
  await new Promise<void>((resolve) => {
    const unmount = mount(root, () => (
      <RemoteAuthScreen label={label.value} pairing={pairing.value} message={message.value} busy={busy.value} />
    ));
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
      busy.value = true;
      message.value = '';
      try {
        if (action === 'request') {
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
          root.removeEventListener('click', clickEvent);
          root.removeEventListener('input', input);
          unmount();
          resolve();
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
