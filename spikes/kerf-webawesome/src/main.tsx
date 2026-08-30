import '@awesome.me/webawesome/dist/styles/webawesome.css';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './style.css';

import { delegate, mount, signal } from 'kerfjs';

type WaInput = HTMLElement & { value: string };
type WaDialog = HTMLElement & { open: boolean; show(): void; hide(): void };

const app = document.querySelector<HTMLElement>('#app');
if (app === null) throw new Error('Missing #app mount point.');
const value = signal('initial');
const structuralRevision = signal(0);
const dark = signal(false);
const events: string[] = [];

function record(name: string): void {
  events.push(name);
  const output = document.querySelector('[data-events]');
  if (output === null) throw new Error('Missing event output.');
  output.textContent = events.join(',');
}

mount(app, () => {
  const revision = structuralRevision.value;
  return (
    <section class={dark.value ? 'wa-dark app' : 'app'} data-revision={revision}>
      <wa-input data-testid="name" label="Name" value={value} hint={`revision ${revision}`}></wa-input>
      <output data-value>{value}</output>
      <output data-events></output>
      <wa-button data-action="rerender">Morph</wa-button>
      <wa-button data-action="theme">Theme</wa-button>
      <wa-button data-action="open" variant="brand">Open dialog</wa-button>
      <wa-dialog data-testid="dialog" label="Confirm">
        Dialog body
        <wa-button slot="footer" data-action="close">Close</wa-button>
      </wa-dialog>
    </section>
  );
});

for (const name of ['input', 'change', 'wa-input', 'wa-change', 'wa-show', 'wa-hide', 'wa-after-show', 'wa-after-hide']) {
  void delegate(app, name, 'wa-input, wa-dialog', (event, target) => {
    record(event.type);
    if (target.matches('wa-input') && (event.type === 'input' || event.type === 'wa-input')) {
      value.value = (target as WaInput).value;
    }
  });
}

void delegate(app, 'click', '[data-action]', (_event, target) => {
  switch ((target as HTMLElement).dataset.action) {
    case 'rerender': structuralRevision.value += 1; break;
    case 'theme': dark.value = !dark.value; break;
    case 'open': (document.querySelector('[data-testid="dialog"]') as WaDialog).show(); break;
    case 'close': (document.querySelector('[data-testid="dialog"]') as WaDialog).hide(); break;
    case undefined: break;
  }
});

Object.assign(globalThis, {
  spike: {
    events,
    input: () => document.querySelector('[data-testid="name"]'),
    dialog: () => document.querySelector('[data-testid="dialog"]'),
  },
});
