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
import { ChoiceCard } from './choice-card';
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
      ? 'Re-export Conversation'
      : draft.writeMode === 'overwrite'
        ? 'Overwrite Export'
        : 'Save Conversation';
  const active = step === 1 ? 'a' : 'b',
    transitionStyle = navigation === 'none' ? 'none' : 'push',
    direction = navigation === 'pop' ? 'backward' : 'forward';
  const scopeScreen = (
    <form {...COMMANDS_AND_AI_ACTIONS.submitConversationExport.attrs}>
      <List gap="l">
        <StepHeader step="Step 1 of 2" title="Choose scope" />
        <fieldset class="conversation-export-dialog__section">
          <legend>Messages</legend>
          <ChoiceCard
            type="radio"
            name="conversation-export-scope"
            value="all"
            checked={!range}
            disabled={busy}
            title="Entire conversation"
            description={`All ${messages.length} messages, in their original order.`}
          />
          <ChoiceCard
            type="radio"
            name="conversation-export-scope"
            value="range"
            checked={range}
            disabled={busy}
            title="Selected range"
            description={`${presetMessages.length} message${presetMessages.length === 1 ? '' : 's'} selected in the chat.`}
          />
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
        <StepHeader step={hasScopeChoice ? 'Step 2 of 2' : 'Save Conversation'} title="Bundle Contents" />
        <fieldset class="conversation-export-dialog__section">
          <legend>Bundle contents</legend>
          <List gap="2xs">
            <ChoiceCard
              type="checkbox"
              name="conversation-export-attachments"
              checked={draft.bundle.includeAttachments}
              disabled={busy}
              title="Attachments"
              description="Copy non-media files referenced by the selected messages."
            />
            <ChoiceCard
              type="checkbox"
              name="conversation-export-media"
              checked={draft.bundle.includeMedia}
              disabled={busy}
              title="Original media"
              description="Copy referenced images, audio, and video at their original quality."
            />
            <ChoiceCard
              type="checkbox"
              name="conversation-export-summary"
              checked={draft.bundle.includeSummary}
              disabled={busy || !summaryAvailable}
              title="Concise summary"
              description={
                summaryAvailable
                  ? 'Add a local summary.md without replacing the complete transcript.'
                  : 'Summary export is unavailable for this conversation.'
              }
            />
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
                  <ChoiceCard
                    type="radio"
                    name="conversation-export-write-mode"
                    value="reexport"
                    checked={draft.writeMode === 'reexport'}
                    disabled={busy}
                    title="Re-export as the next revision"
                    description={`Keep lineage to revision ${existing.revision} in the manifest.`}
                  />
                )}
                <ChoiceCard
                  type="radio"
                  name="conversation-export-write-mode"
                  value="overwrite"
                  checked={draft.writeMode === 'overwrite'}
                  disabled={busy}
                  tone="danger"
                  title="Overwrite the existing bundle"
                  description="Replace its files in this destination."
                />
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
          label="Save Conversation"
          open
          with-footer
        >
          <ContentTransition
            active={active}
            style={transitionStyle}
            direction={direction}
            label="Save Conversation Navigation"
            a={scopeScreen}
            b={bundleScreen}
          />
          <ContentTransition
            active={active}
            style="crossfade"
            direction={direction}
            region="footer"
            sideLayout="actions"
            label="Save Conversation Actions"
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
