import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/tag/tag.js';
import './conversation-export-dialog.css';

import { ContentItem } from '@kerfjs/ui/content-item';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import type { ConversationActivity, ConversationMessage } from '../ai-conversation';
import {
  type ConversationExportDraft,
  type ConversationExportScope,
  type ConversationExportSource,
  conversationExportValidation,
  selectedConversationMessages,
} from '../conversation-export';
import { COMMANDS_AND_AI_ACTIONS, COMMANDS_AND_AI_TARGETS } from '../interaction-attrs/commands-and-ai';
import { ContentTransition } from './content-transition';
import { FlowBackButton } from './flow-back-button';

export interface ConversationExportDialogState {
  source: ConversationExportSource;
  messages: readonly ConversationMessage[];
  activity?: readonly ConversationActivity[];
  draft: ConversationExportDraft;
  busy?: boolean;
  error?: string;
  summaryAvailable?: boolean;
  step?: 1 | 2;
  navigation?: 'none' | 'push' | 'pop';
  selectedRange?: Extract<ConversationExportScope, { kind: 'range' }>;
}

export function ConversationExportDialog({ state }: { state?: ConversationExportDialogState }) {
  if (!state) return <></>;
  const {
    source,
    messages,
    draft,
    busy = false,
    error = '',
    summaryAvailable = true,
    step = 1,
    navigation = 'none',
    selectedRange,
  } = state;
  const range = draft.scope.kind === 'range';
  const selectedMessages = selectedConversationMessages(messages, draft.scope);
  const presetMessages = selectedRange ? selectedConversationMessages(messages, selectedRange) : [];
  const hasScopeChoice = Boolean(selectedRange);
  const existing = draft.destination?.existing;
  const sameConversation = existing?.sourceConversationId === source.conversationId;
  const validation = conversationExportValidation(messages, draft, source);
  const blockingValidation = validation === 'Choose where to save the conversation.' ? undefined : validation;
  const summaryUnavailable = draft.bundle.includeSummary && !summaryAvailable;
  const submitLabel = busy
    ? 'Saving…'
    : draft.writeMode === 'reexport'
      ? 'Re-export conversation'
      : draft.writeMode === 'overwrite'
        ? 'Overwrite export'
        : 'Save conversation';
  const active = step === 1 ? 'a' : 'b',
    transitionStyle = navigation === 'none' ? 'none' : 'push',
    direction = navigation === 'pop' ? 'backward' : 'forward';
  const scopeScreen = (
    <form {...COMMANDS_AND_AI_ACTIONS.submitConversationExport.attrs}>
      <List gap="l">
        <StepHeader step="Step 1 of 2" title="Choose scope" />
        <fieldset class="conversation-export-dialog__section">
          <legend>Messages</legend>
          <label class="conversation-export-dialog__choice">
            <input type="radio" name="conversation-export-scope" value="all" checked={!range} disabled={busy} />
            <span>
              <strong>Entire conversation</strong>
              <small>All {messages.length} messages, in their original order.</small>
            </span>
          </label>
          <label class="conversation-export-dialog__choice">
            <input type="radio" name="conversation-export-scope" value="range" checked={range} disabled={busy} />
            <span>
              <strong>Selected range</strong>
              <small>
                {presetMessages.length} message{presetMessages.length === 1 ? '' : 's'} selected in the chat.
              </small>
            </span>
          </label>
        </fieldset>
        <ExportError message={step === 1 ? error : ''} />
      </List>
    </form>
  );
  const bundleScreen = (
    <form {...COMMANDS_AND_AI_ACTIONS.submitConversationExport.attrs}>
      <List gap="l">
        {hasScopeChoice && (
          <FlowBackButton action="previous-conversation-export-step" label="Message scope" disabled={busy} />
        )}
        <StepHeader step={hasScopeChoice ? 'Step 2 of 2' : 'Save conversation'} title="Bundle contents" />
        <fieldset class="conversation-export-dialog__section">
          <legend>Bundle contents</legend>
          <List gap="2xs">
            <label class="conversation-export-dialog__choice">
              <input
                type="checkbox"
                name="conversation-export-attachments"
                checked={draft.bundle.includeAttachments}
                disabled={busy}
              />
              <span>
                <strong>Attachments</strong>
                <small>Copy non-media files referenced by the selected messages.</small>
              </span>
            </label>
            <label class="conversation-export-dialog__choice">
              <input
                type="checkbox"
                name="conversation-export-media"
                checked={draft.bundle.includeMedia}
                disabled={busy}
              />
              <span>
                <strong>Original media</strong>
                <small>Copy referenced images, audio, and video at their original quality.</small>
              </span>
            </label>
            <label class="conversation-export-dialog__choice">
              <input
                type="checkbox"
                name="conversation-export-summary"
                checked={draft.bundle.includeSummary}
                disabled={busy || !summaryAvailable}
              />
              <span>
                <strong>Concise summary</strong>
                <small>
                  {summaryAvailable
                    ? 'Add a local summary.md without replacing the complete transcript.'
                    : 'Summary export is unavailable for this conversation.'}
                </small>
              </span>
            </label>
          </List>
          {existing && (
            <ContentItem appearance="warning">
              <List gap="2xs">
                <Row>
                  <wa-tag variant="warning">Existing export</wa-tag>
                </Row>
                <Text flush size="compact" role="status">
                  {sameConversation
                    ? `Revision ${existing.revision} of this conversation is already there.`
                    : 'This destination contains a different conversation export.'}
                </Text>
                {sameConversation && (
                  <label class="conversation-export-dialog__choice">
                    <input
                      type="radio"
                      name="conversation-export-write-mode"
                      value="reexport"
                      checked={draft.writeMode === 'reexport'}
                      disabled={busy}
                    />
                    <span>
                      <strong>Re-export as the next revision</strong>
                      <small>Keep lineage to revision {existing.revision} in the manifest.</small>
                    </span>
                  </label>
                )}
                <label class="conversation-export-dialog__choice conversation-export-dialog__choice--danger">
                  <input
                    type="radio"
                    name="conversation-export-write-mode"
                    value="overwrite"
                    checked={draft.writeMode === 'overwrite'}
                    disabled={busy}
                  />
                  <span>
                    <strong>Overwrite the existing bundle</strong>
                    <small>Replace its files in this destination.</small>
                  </span>
                </label>
              </List>
            </ContentItem>
          )}
        </fieldset>
        <ExportError
          message={
            step === 2
              ? error ||
                (summaryUnavailable ? 'Summary export is unavailable for this conversation.' : blockingValidation) ||
                ''
              : ''
          }
        />
      </List>
    </form>
  );
  const scopeActions = (
    <>
      <wa-button appearance="plain" type="button" data-action="cancel-conversation-export" disabled={busy}>
        Cancel
      </wa-button>
      <wa-button
        appearance="accent"
        type="button"
        data-action="next-conversation-export-step"
        disabled={busy || selectedMessages.length === 0}
      >
        Continue
      </wa-button>
    </>
  );
  const bundleActions = (
    <>
      <wa-button appearance="plain" type="button" data-action="cancel-conversation-export" disabled={busy}>
        Cancel
      </wa-button>
      <wa-button
        appearance="accent"
        type="button"
        data-action="finish-conversation-export"
        disabled={busy || Boolean(blockingValidation) || summaryUnavailable}
      >
        {submitLabel}
      </wa-button>
    </>
  );

  return (
    <div data-key="conversation-export-dialog">
      <DialogSurface preferredWidth={remify(624)} viewportGutter={remify(16)} maxHeight={remify(704)}>
        <wa-dialog
          data-key="conversation-export-dialog"
          class="conversation-export-dialog"
          {...COMMANDS_AND_AI_TARGETS.conversationExportDialog.attrs}
          data-step={step}
          data-navigation={navigation}
          label="Save conversation"
          open
          with-footer
        >
          <ContentTransition
            active={active}
            style={transitionStyle}
            direction={direction}
            label="Save conversation navigation"
            a={scopeScreen}
            b={bundleScreen}
          />
          <ContentTransition
            active={active}
            style="crossfade"
            direction={direction}
            region="footer"
            sideLayout="actions"
            label="Save conversation actions"
            a={scopeActions}
            b={bundleActions}
          />
        </wa-dialog>
      </DialogSurface>
    </div>
  );
}

function StepHeader({ step, title }: { step: string; title: string }) {
  return (
    <List gap="2xs">
      <Text flush variant="span" size="compact" tone="quiet">
        {step}
      </Text>
      <Text flush variant="h3" size="large">
        {title}
      </Text>
    </List>
  );
}

/** The alert region stays mounted so a new message announces; an empty message keeps the region empty. */
function ExportError({ message }: { message: string }) {
  return (
    <div class="conversation-export-dialog__error" role="alert">
      {message && (
        <Text flush size="compact" tone="danger">
          {message}
        </Text>
      )}
    </div>
  );
}
