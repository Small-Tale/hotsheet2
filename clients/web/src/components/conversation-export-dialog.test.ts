import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { defaultConversationExportDraft } from '../conversation-export';
import { ConversationExportDialog } from './conversation-export-dialog';

const source = { conversationId: 'conversation-1', sessionId: 'session-1', tool: 'Codex' };
const messages = [
  { id: 'message-1', role: 'user' as const, content: 'Plan the release.' },
  { id: 'message-2', role: 'assistant' as const, content: 'I will inspect the release checks.' },
  { id: 'message-3', role: 'user' as const, content: 'Only include verified changes.' },
];

describe('ConversationExportDialog', () => {
  it('uses Kerf semantic spacing for dialog regions, choices, and connected labels', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'conversation-export-dialog.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    // The option cards are the shared ChoiceCard now (HS2-V7PCS6).
    expect(css).not.toContain('__choice');
    const choiceCss = readFileSync(resolve(import.meta.dirname, 'choice-card.css'), 'utf8');
    expect(choiceCss).toContain('padding: var(--kui-space-xs)');
    expect(choiceCss).toContainSource('gap:var(--kui-space-2xs)');
    // DialogSurface owns the shell and Kerf List/Text/ContentItem own the layout (HS2-3Q6FXF).
    expect(css).not.toMatch(/::part|@media|__form|__header|__options|__collision|__screen/);
    const markup = String(
      ConversationExportDialog({ state: { source, messages, draft: defaultConversationExportDraft(), step: 2 } }),
    );
    expect(markup).toContain('data-component="dialog-surface"');
    expect(markup).toContain('data-preferred-width');
    expect(markup).toContain('<div class="conversation-export-dialog__error" role="alert"></div>');
  });

  it('skips message scope when the chat has no selection', () => {
    const markup = String(
      ConversationExportDialog({ state: { source, messages, draft: defaultConversationExportDraft(), step: 2 } }),
    );
    expect(markup).toContain('data-dialog-component="conversation-export-dialog"');
    expect(markup).toContain('data-step="2"');
    expect(markup).toContain('data-navigation="none"');
    expect(markup).toContain('data-active-side="b"');
    expect(markup).toContain('data-transition-style="none"');
    expect(markup).toContain('Save Conversation');
    expect(markup).toContain('Bundle contents');
    expect(markup).toContain('data-action="submit-conversation-export"');
    expect(markup).not.toContain('previous-conversation-export-step');
  });

  it('offers the messages already selected in the chat without asking users to pick it again', () => {
    const selectedScope = { kind: 'messages' as const, messageIds: ['message-2', 'message-3'] };
    const markup = String(
      ConversationExportDialog({
        state: {
          source,
          messages,
          draft: { ...defaultConversationExportDraft(), scope: selectedScope },
          selectedScope,
          step: 1,
        },
      }),
    );
    expect(markup).toContain('data-step="1"');
    expect(markup).toContain('data-active-side="a"');
    expect(markup).toContain('Step 1 of 2');
    expect(markup).toContain('Choose scope');
    expect(markup).toContain('name="conversation-export-scope" value="all"');
    expect(markup).not.toContain('name="conversation-export-scope" value="all" checked');
    expect(markup).toContain('name="conversation-export-scope" value="selected" checked');
    expect(markup).toContain('2 messages selected in the chat.');
    expect(markup).not.toContain('class="ai-conversation__message');
    expect(markup).not.toContain('pick-conversation-export-message');
    expect(markup).toContain('data-action="next-conversation-export-step"');
  });

  it('pushes forward and pops backward with the shared in-content back affordance', () => {
    const selectedScope = { kind: 'messages' as const, messageIds: ['message-2', 'message-3'] };
    const forward = String(
      ConversationExportDialog({
        state: {
          source,
          messages,
          draft: { ...defaultConversationExportDraft(), scope: selectedScope },
          selectedScope,
          step: 2,
          navigation: 'push',
        },
      }),
    );
    expect(forward).toContain('data-navigation="push"');
    expect(forward).toContain('data-transition-style="push"');
    expect(forward).toContain('data-transition-direction="forward"');
    expect(forward).toContain('flow-back-button');
    expect(forward).toContain('data-action="previous-conversation-export-step"');
    expect(forward).toContain('data-lucide="chevron-left"');
    expect(forward).toContain('Message scope');
    const backward = String(
      ConversationExportDialog({
        state: {
          source,
          messages,
          draft: { ...defaultConversationExportDraft(), scope: selectedScope },
          selectedScope,
          step: 1,
          navigation: 'pop',
        },
      }),
    );
    expect(backward).toContain('data-navigation="pop"');
    expect(backward).toContain('data-transition-direction="backward"');
  });

  it('reviews a selected-messages scope and a same-conversation re-export choice', () => {
    const selectedScope = { kind: 'messages' as const, messageIds: ['message-2', 'message-3'] };
    const markup = String(
      ConversationExportDialog({
        state: {
          source,
          messages,
          draft: {
            scope: selectedScope,
            destination: {
              selectionToken: 'opaque-selection',
              displayPath: '/Users/me/Exports/release-review',
              kind: 'directory',
              existing: { exportId: 'export-1', revision: 4, sourceConversationId: 'conversation-1' },
            },
            writeMode: 'reexport',
            bundle: { ...defaultConversationExportDraft().bundle, includeSummary: true },
          },
          step: 2,
          selectedScope,
        },
      }),
    );
    expect(markup).toContain('Step 2 of 2');
    expect(markup).toContain('Revision 4 of this conversation is already there.');
    expect(markup).toContain('name="conversation-export-write-mode" value="reexport" checked');
    expect(markup).toContain('name="conversation-export-write-mode" value="overwrite"');
    expect(markup).toContain('Re-export Conversation');
    expect(markup).not.toContain('data-action="finish-conversation-export" disabled');
  });

  it('requires overwrite for another conversation and explains a non-resumable source', () => {
    const markup = String(
      ConversationExportDialog({
        state: {
          source: { conversationId: 'conversation-1', tool: 'Codex' },
          messages,
          step: 2,
          draft: {
            ...defaultConversationExportDraft(),
            destination: {
              selectionToken: 'opaque-selection',
              displayPath: '/Exports/existing',
              kind: 'archive',
              existing: { exportId: 'export-2', revision: 1, sourceConversationId: 'conversation-2' },
            },
          },
        },
      }),
    );
    expect(markup).toContain('This destination contains a different conversation export.');
    expect(markup).not.toContain('value="reexport"');
    expect(markup).toContain('value="overwrite"');
    expect(markup).toContain('Choose re-export or overwrite for the existing bundle.');
  });

  it('disables summary selection and submission when a stale summary choice is unavailable', () => {
    const markup = String(
      ConversationExportDialog({
        state: {
          source,
          messages,
          step: 2,
          summaryAvailable: false,
          draft: {
            ...defaultConversationExportDraft(),
            destination: { selectionToken: 'opaque-selection', displayPath: '/Exports/new', kind: 'directory' },
            bundle: { ...defaultConversationExportDraft().bundle, includeSummary: true },
          },
        },
      }),
    );
    expect(markup).toContain('name="conversation-export-summary" checked disabled');
    expect(markup).toContain('Summary export is unavailable for this conversation.');
  });

  it('defers destination choice to the final save action', () => {
    const markup = String(
      ConversationExportDialog({ state: { source, messages, draft: defaultConversationExportDraft(), step: 2 } }),
    );
    expect(markup).not.toContain('Step 2 of 2');
    expect(markup).toContain('Save Conversation');
    expect(markup).not.toContain('Choose where to save the conversation.');
    expect(markup).not.toContain('disabled>Save Conversation');
    expect(markup).not.toContain('manifest.json');
  });
});
