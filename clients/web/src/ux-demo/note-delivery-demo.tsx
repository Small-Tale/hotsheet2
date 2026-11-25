import { CatalogExample } from '@kerfjs/ui/catalog';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { NoteDeliveryPopup } from '../components/note-delivery-popup';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { DemoEvent } from './components/demo-event';
import { selectedId } from './components/demo-state';
import type { DemoModule } from './demo-module';

export const noteDeliveryDemoKind = signal<'choose' | 'start' | undefined>('choose'),
  noteDeliveryDemoResult = signal('Your note was saved. Choose the session to notify.');

/** The production prompt over synthetic sessions (HS2-6RXBS9). */
export function NoteDeliveryPopupDemo() {
  const kind = noteDeliveryDemoKind.value;
  return (
    <CatalogExample label="Interactive" align="glyph">
      <DemoEvent>{noteDeliveryDemoResult.value}</DemoEvent>
      <button type="button" data-note-delivery-demo="choose">
        Show Choose
      </button>
      <button type="button" data-note-delivery-demo="start">
        Show Start
      </button>
      {kind && (
        <NoteDeliveryPopup
          prompt={{
            key: `demo-note-delivery-${kind}`,
            ticketId: 'git:demo-1',
            slug: 'HS2-DEMO01',
            title: 'Synthetic ticket awaiting feedback',
            projectName: 'Hot Sheet demo',
            kind,
            sessions:
              kind === 'choose'
                ? [
                    {
                      id: 'session-a',
                      label: 'Claude 1 · Hot Sheet demo · HS2-DEMO01',
                      description: 'Claude in Hot Sheet demo (session session-a)',
                    },
                    {
                      id: 'session-b',
                      label: 'Codex chat · Hot Sheet demo',
                      description: 'Codex in Hot Sheet demo, no claimed ticket (session session-b)',
                    },
                  ]
                : [],
          }}
        />
      )}
    </CatalogExample>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireNoteDeliveryDemo(root: HTMLElement): () => void {
  const scope = createScope();
  scope.add(
    delegate(root, 'click', '[data-note-delivery-demo]', (_event, target) => {
      noteDeliveryDemoKind.value = (target as HTMLElement).dataset.noteDeliveryDemo === 'start' ? 'start' : 'choose';
    }),
  );
  for (const [action, result] of [
    [NOTIFICATIONS_AND_LINKS_ACTIONS.chooseFeedbackSession, 'The chosen session was notified.'],
    [NOTIFICATIONS_AND_LINKS_ACTIONS.startFeedbackSession, 'A session starts in the note’s project.'],
    [NOTIFICATIONS_AND_LINKS_ACTIONS.dismissFeedbackDelivery, 'Dismissed. The note stays for the AI’s next read.'],
  ] as const)
    scope.add(
      delegate(root, 'click', action.selector, () => {
        if (selectedId.value !== 'note-delivery-popup') return;
        noteDeliveryDemoResult.value = result;
        noteDeliveryDemoKind.value = undefined;
      }),
    );
  return () => {
    scope.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'note-delivery-popup': { catalogExamples: true, Demo: NoteDeliveryPopupDemo },
  },
  wire: wireNoteDeliveryDemo,
};
