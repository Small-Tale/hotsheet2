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
import { terminalQuestionAnswers } from '../terminal-question-answer';
import { TOP_LAYER_PRESENTED_EVENT } from '../top-layer-overlay';

export function createHaltedSessionsController(dependencies: {
  episodes: () => HaltedSessionEpisode[];
  unresolvedProjects: () => string[];
  restoring: () => boolean;
  paused: () => boolean;
  permissionVisible: () => boolean;
  open: (episode: HaltedSessionEpisode) => void;
  answer: (episode: HaltedSessionEpisode, answers?: Record<string, string>) => Promise<void>;
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
  lifetime.add(
    delegate(
      document.body,
      'submit',
      NOTIFICATIONS_AND_LINKS_ACTIONS.answerTerminalQuestion.selector,
      (event, target) => {
        event.preventDefault();
        const form = target as HTMLFormElement,
          key = form.dataset.haltKey,
          episode = key ? inbox.find(key) : undefined,
          questions = episode?.question?.questions,
          error = form.querySelector<HTMLElement>('.halted-session-popup__answer-error'),
          submit = form.querySelector<HTMLButtonElement>('button[type="submit"]');
        if (!episode || !questions || !error || !submit) return;
        const answers = terminalQuestionAnswers(questions, new FormData(form));
        if (!answers) {
          error.textContent = 'Answer every question, including any selected Other option.';
          error.hidden = false;
          return;
        }
        submit.disabled = true;
        error.hidden = true;
        void dependencies.answer(episode, answers).then(
          () => {
            inbox.dismiss(episode.key);
            update();
          },
          (reason: unknown) => {
            submit.disabled = false;
            error.textContent = reason instanceof Error ? reason.message : 'Could not send the answer.';
            error.hidden = false;
          },
        );
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'input',
      `${NOTIFICATIONS_AND_LINKS_ACTIONS.answerTerminalQuestion.selector} input`,
      (_event, target) => {
        const input = target as HTMLInputElement,
          form = input.closest('form'),
          error = form?.querySelector<HTMLElement>('.halted-session-popup__answer-error');
        if (input.name.startsWith('other-') && input.value.trim()) {
          const index = input.name.slice('other-'.length),
            other = form?.querySelector<HTMLInputElement>(`input[name="choice-${index}"][value="other"]`);
          if (other) other.checked = true;
        }
        if (error) error.hidden = true;
      },
    ),
  );
  lifetime.add(
    delegate(
      document.body,
      'click',
      NOTIFICATIONS_AND_LINKS_ACTIONS.returnTerminalQuestion.selector,
      (_event, target) => {
        const key = (target as HTMLElement).dataset.haltKey,
          episode = key ? inbox.find(key) : undefined;
        if (!episode?.question) return;
        void dependencies.answer(episode).then(
          () => {
            inbox.dismiss(episode.key);
            dependencies.open(episode);
            update();
          },
          () => {
            dependencies.open(episode);
          },
        );
      },
    ),
  );
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
