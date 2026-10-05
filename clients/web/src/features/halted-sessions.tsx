import { delegate, effect, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { HaltedSessionPopup } from '../components/halted-session-popup';
import {
  HALTED_SESSION_SEEN_KEY,
  type HaltedSessionEpisode,
  HaltedSessionInbox,
  loadHaltedSessionSeen,
  persistHaltedSessionSeen,
  synchronizeHaltedSessionSeen,
} from '../halted-sessions';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import { TOP_LAYER_PRESENTED_EVENT } from '../top-layer-overlay';

export function createHaltedSessionsController(dependencies: {
  episodes: () => HaltedSessionEpisode[];
  unresolvedProjects: () => string[];
  restoring: () => boolean;
  paused: () => boolean;
  permissionVisible: () => boolean;
  open: (episode: HaltedSessionEpisode) => void;
}) {
  let storage: Storage | undefined;
  try {
    storage = globalThis.localStorage;
  } catch {
    /* In-memory dedupe still works. */
  }
  const inbox = new HaltedSessionInbox(loadHaltedSessionSeen(storage)),
    revision = signal(0),
    lifetime = createScope();
  const update = () => {
    revision.value = revision.peek() + 1;
  };
  let projection = '';
  lifetime.add(
    effect(() => {
      const episodes = dependencies.episodes(),
        unresolvedProjects = dependencies.unresolvedProjects(),
        restoring = dependencies.restoring(),
        next = JSON.stringify([episodes, unresolvedProjects, restoring]);
      if (next === projection) return;
      projection = next;
      inbox.reconcile(episodes, unresolvedProjects, restoring);
      update();
    }),
  );
  const acknowledge = (event: Event) => {
    const key = (event.target as HTMLElement).dataset.haltKey;
    if (!key || !inbox.presented(key)) return;
    persistHaltedSessionSeen(inbox, storage);
    // Presentation happens during the top-layer mutation watcher; render after it settles.
    queueMicrotask(update);
  };
  document.body.addEventListener(TOP_LAYER_PRESENTED_EVENT, acknowledge);
  lifetime.add(() => {
    document.body.removeEventListener(TOP_LAYER_PRESENTED_EVENT, acknowledge);
  });
  const sync = (event: StorageEvent) => {
    if (event.key !== HALTED_SESSION_SEEN_KEY || (event.storageArea && event.storageArea !== storage)) return;
    synchronizeHaltedSessionSeen(inbox, event.newValue, storage);
    update();
  };
  window.addEventListener('storage', sync);
  lifetime.add(() => {
    window.removeEventListener('storage', sync);
  });
  for (const [action, open] of [
    [NOTIFICATIONS_AND_LINKS_ACTIONS.dismissHaltedSession, false],
    [NOTIFICATIONS_AND_LINKS_ACTIONS.openHaltedSession, true],
  ] as const) {
    lifetime.add(
      delegate(document.body, 'click', action.selector, (_event, target) => {
        const key = (target as HTMLElement).dataset.haltKey,
          episode = key ? inbox.find(key) : undefined;
        if (!episode) return;
        inbox.dismiss(episode.key);
        if (open) dependencies.open(episode);
        update();
      }),
    );
  }
  function popup() {
    void revision.value;
    const episode = inbox.visible(dependencies.paused(), dependencies.permissionVisible());
    return episode ? <HaltedSessionPopup episode={episode} /> : undefined;
  }
  return {
    popup,
    waiting: () => inbox.waiting(),
    dispose: () => {
      lifetime.dispose();
    },
  };
}
