import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { HaltedSessionPopup } from '../components/halted-session-popup';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { DemoEvent } from './components/demo-event';
import { selectedId } from './components/demo-state';
import type { DemoModule } from './demo-module';

export const haltedSessionDemoVisible = signal(true),
  haltedSessionDemoResult = signal('The session remains halted after dismissing this prompt.');

export function HaltedSessionPopupDemo() {
  return (
    <CatalogExample label="Interactive" align="glyph">
      <DemoEvent>{haltedSessionDemoResult.value}</DemoEvent>
      <button type="button" data-reset-halt-demo>
        Show halted session
      </button>
      {haltedSessionDemoVisible.value && (
        <HaltedSessionPopup
          episode={{
            key: 'demo-halt',
            kind: 'terminal',
            projectId: 'demo',
            projectName: 'Hot Sheet demo',
            sessionId: 'claude-main',
            sessionName: 'Claude main worker',
            message: 'Selected model is at capacity. Please try a different model.',
          }}
        />
      )}
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireHaltedSessionDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  void delegate(root, 'click', '[data-reset-halt-demo]', () => {
    haltedSessionDemoVisible.value = true;
  });
  for (const [action, result] of [
    [
      NOTIFICATIONS_AND_LINKS_ACTIONS.openHaltedSession,
      'Open session returns to the originating project and session without clearing its halt indicator.',
    ],
    [NOTIFICATIONS_AND_LINKS_ACTIONS.dismissHaltedSession, 'Prompt dismissed. The session remains halted.'],
    [
      NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications,
      'Notifications paused. Unseen halted sessions wait until Resume.',
    ],
  ] as const)
    void delegate(root, 'click', action.selector, () => {
      if (selectedId.value !== 'halted-session-popup') return;
      haltedSessionDemoResult.value = result;
      haltedSessionDemoVisible.value = false;
    });
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'halted-session-popup': { catalogExamples: true, Demo: HaltedSessionPopupDemo },
  },
  wire: wireHaltedSessionDemo,
};
