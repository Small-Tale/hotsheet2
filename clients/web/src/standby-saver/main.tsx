import '@kerfjs/ui/foundation.css';
import '@kerfjs/ui/document.css';
import '../hot-sheet-tokens.css';

import { mount, signal } from 'kerfjs';

import { StandbyScreen } from '../components/standby-screen';
import { parseStandbySnapshot, saverView, type StandbySnapshot } from './snapshot';

/**
 * The macOS screen saver's page (HS2-MZM988, docs/34-glance-surfaces.md §34.12): the real `StandbyScreen`,
 * inline, over the live state the Hot Sheet server leaves in a snapshot file. The native saver view
 * calls `window.hotsheetSaver.update(snapshot)` when the file changes; the page makes no request. Ages
 * and the clock re-render from a local display timer.
 */
const CLOCK_TICK_MS = 15_000;

const snapshot = signal<StandbySnapshot | undefined>(undefined);
const nowMs = signal(Date.now());

window.setInterval(() => {
  nowMs.value = Date.now();
}, CLOCK_TICK_MS);

(window as unknown as { hotsheetSaver: unknown }).hotsheetSaver = {
  /** Take the newest snapshot; anything that does not parse leaves the last good one showing. */
  update(raw: unknown) {
    try {
      snapshot.value = parseStandbySnapshot(raw);
      nowMs.value = Date.now();
    } catch (reason) {
      console.error('Ignored an invalid standby snapshot', reason);
    }
  },
};

mount(document.getElementById('app')!, () => {
  const view = saverView(snapshot.value, nowMs.value);
  return <StandbyScreen clock={view.clock} model={view.model} freshness={view.freshness} presentation="inline" />;
});
