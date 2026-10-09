import './halted-session-popup.css';

import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bot, CircleAlert, CircleHelp } from 'lucide';

import type { HaltedSessionEpisode } from '../halted-sessions';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import {
  TOP_LAYER_DISMISS_ATTRIBUTE,
  TOP_LAYER_OVERLAY_ATTRIBUTE,
  TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE,
} from '../top-layer-overlay';

/** A native top-layer shell around the same compact card pattern as permission popups. */
export function HaltedSessionPopup({ episode }: { episode: HaltedSessionEpisode }) {
  return (
    <dialog
      class="halted-session-popup"
      data-component="halted-session-popup"
      data-kind={episode.kind}
      data-halt-key={episode.key}
      popover="manual"
      {...{ [TOP_LAYER_OVERLAY_ATTRIBUTE]: '' }}
      {...{ [TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE]: episode.key }}
      tabindex={-1}
      aria-label={episode.kind === 'question' ? 'AI waiting for your answer' : 'AI session halted'}
    >
      <article class="halted-session-popup__card">
        <header class="halted-session-popup__header">
          <span class="halted-session-popup__identity">
            <LucideIcon icon={Bot} name="bot" size="s" color={foregroundColorVar('--wa-color-success-fill-loud')} />
            <strong>{episode.sessionName}</strong>
          </span>
          <span class="halted-session-popup__project" title={episode.projectName}>
            {episode.projectName}
          </span>
        </header>
        <div class="halted-session-popup__summary">
          <LucideIcon
            icon={episode.kind === 'question' ? CircleHelp : CircleAlert}
            name={episode.kind === 'question' ? 'circle-help' : 'circle-alert'}
            size={17.6}
            color={foregroundColorVar(
              episode.kind === 'question' ? '--wa-color-brand-fill-loud' : '--wa-color-danger-fill-loud',
            )}
          />
          <strong>{episode.kind === 'question' ? 'AI waiting for your answer' : 'AI session halted'}</strong>
        </div>
        {episode.kind === 'question' && episode.question?.questions?.length ? (
          <form
            class="halted-session-popup__questions"
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.answerTerminalQuestion.attrs}
            data-halt-key={episode.key}
          >
            {episode.question.questions.map((question, index) => (
              <fieldset class="halted-session-popup__question">
                <legend>{question.question}</legend>
                {(question.options ?? []).map((option, optionIndex) => (
                  <label class="halted-session-popup__option">
                    <input
                      type={question.multiSelect ? 'checkbox' : 'radio'}
                      name={`choice-${index}`}
                      value={String(optionIndex)}
                    />
                    <span>
                      <strong>{option.label}</strong>
                      {option.description ? <small>{option.description}</small> : null}
                    </span>
                  </label>
                ))}
                {(question.options ?? []).length ? (
                  <label class="halted-session-popup__option">
                    <input type={question.multiSelect ? 'checkbox' : 'radio'} name={`choice-${index}`} value="other" />
                    <span>Other</span>
                  </label>
                ) : null}
                <input
                  class="halted-session-popup__free-text"
                  type="text"
                  name={`other-${index}`}
                  aria-label={`Other answer for ${question.question}`}
                  placeholder="Enter your answer"
                  maxLength={2000}
                />
              </fieldset>
            ))}
            <p class="halted-session-popup__answer-error" role="alert" hidden />
            <button class="halted-session-popup__primary" type="submit">
              Send answer
            </button>
          </form>
        ) : (
          <pre class="halted-session-popup__details">
            <code>{episode.message}</code>
          </pre>
        )}
        <footer class="halted-session-popup__footer">
          <div class="halted-session-popup__quiet-actions">
            <button
              type="button"
              {...NOTIFICATIONS_AND_LINKS_ACTIONS.dismissHaltedSession.attrs}
              data-halt-key={episode.key}
              {...{ [TOP_LAYER_DISMISS_ATTRIBUTE]: '' }}
            >
              Dismiss
            </button>
            <button type="button" {...NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications.attrs}>
              Pause notifications
            </button>
          </div>
          <button
            class="halted-session-popup__primary"
            type="button"
            {...(episode.kind === 'question' && episode.question?.questions?.length
              ? NOTIFICATIONS_AND_LINKS_ACTIONS.returnTerminalQuestion.attrs
              : NOTIFICATIONS_AND_LINKS_ACTIONS.openHaltedSession.attrs)}
            data-halt-key={episode.key}
          >
            {episode.kind === 'question' && episode.question?.questions?.length ? 'Answer in terminal' : 'Open session'}
          </button>
        </footer>
      </article>
    </dialog>
  );
}
