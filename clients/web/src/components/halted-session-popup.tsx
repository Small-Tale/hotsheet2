import './halted-session-popup.css';

import { Button } from '@kerfjs/ui/button';
import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { Row } from '@kerfjs/ui/row';
import { CircleAlert, CircleHelp } from 'lucide';

import type { HaltedSessionEpisode } from '../halted-sessions';
import { NOTIFICATIONS_AND_LINKS_ACTIONS } from '../interaction-attrs/notifications-and-links';
import {
  TOP_LAYER_DISMISS_ATTRIBUTE,
  TOP_LAYER_OVERLAY_ATTRIBUTE,
  TOP_LAYER_PRESENTATION_KEY_ATTRIBUTE,
} from '../top-layer-overlay';
import { NotificationCard } from './notification-card';

/** A native top-layer shell around the same compact card pattern as permission popups. */
export function HaltedSessionPopup({ episode }: { episode: HaltedSessionEpisode }) {
  const question = episode.kind === 'question',
    questions = (question && episode.question?.questions) || [],
    answerable = questions.length > 0;
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
      <NotificationCard
        elevation="popup"
        identity={episode.sessionName}
        projectName={episode.projectName}
        summary={{
          icon: question ? CircleHelp : CircleAlert,
          iconName: question ? 'circle-help' : 'circle-alert',
          label: question ? 'AI waiting for your answer' : 'AI session halted',
          color: foregroundColorVar(question ? '--wa-color-brand-fill-loud' : '--wa-color-danger-fill-loud'),
        }}
        details={answerable ? undefined : { text: episode.message, appearance: question ? 'prose' : 'code' }}
        quietActions={
          <>
            <Button
              emphasis="plain"
              size="compact"
              action={NOTIFICATIONS_AND_LINKS_ACTIONS.dismissHaltedSession.value}
              rootAttributes={{ 'data-halt-key': episode.key, ...{ [TOP_LAYER_DISMISS_ATTRIBUTE]: '' } }}
            >
              Dismiss
            </Button>
            <Button emphasis="plain" size="compact" action={NOTIFICATIONS_AND_LINKS_ACTIONS.pauseNotifications.value}>
              Pause Notifications
            </Button>
          </>
        }
        endActions={
          <Button
            className={'halted-session-popup__primary'}
            size="compact"
            emphasis={answerable ? 'secondary' : 'primary'}
            tone={answerable ? 'neutral' : 'brand'}
            action={
              answerable
                ? NOTIFICATIONS_AND_LINKS_ACTIONS.returnTerminalQuestion.value
                : NOTIFICATIONS_AND_LINKS_ACTIONS.openHaltedSession.value
            }
            rootAttributes={{ 'data-halt-key': episode.key }}
          >
            {answerable ? 'Answer in terminal' : 'Open session'}
          </Button>
        }
      >
        {answerable ? (
          <form
            class="halted-session-popup__questions"
            {...NOTIFICATIONS_AND_LINKS_ACTIONS.answerTerminalQuestion.attrs}
            data-halt-key={episode.key}
          >
            {questions.map((question, index) => (
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
            <Row hAlign="right">
              <Button
                className={'halted-session-popup__send'}
                tone={'brand'}
                emphasis="primary"
                size="compact"
                type="submit"
              >
                Send Answer
              </Button>
            </Row>
          </form>
        ) : undefined}
      </NotificationCard>
    </dialog>
  );
}
