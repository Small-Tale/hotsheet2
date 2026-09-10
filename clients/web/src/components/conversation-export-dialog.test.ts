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
  it('renders the all/range, destination, bundle, and summary contracts', () => {
    const markup = String(ConversationExportDialog({ state: { source, messages, draft: defaultConversationExportDraft() } }));
    expect(markup).toContain('data-component="conversation-export-dialog"');
    expect(markup).toContain('data-action="submit-conversation-export"');
    expect(markup).toContain('name="conversation-export-scope" value="all" checked');
    expect(markup).toContain('name="conversation-export-scope" value="range"');
    expect(markup).toContain('name="conversation-export-start-message"');
    expect(markup).toContain('name="conversation-export-end-message"');
    expect(markup).toContain('data-action="pick-conversation-export-destination"');
    expect(markup).toContain('name="conversation-export-attachments" checked');
    expect(markup).toContain('name="conversation-export-media" checked');
    expect(markup).toContain('data-bundle-purpose="structure"');
    expect(markup).toContain('name="conversation-export-summary"');
    expect(markup).toContain('summary.md');
    expect(markup).not.toContain('data-bundle-purpose="summary"');
    expect(markup).toContain('Reopen metadata is always included.');
    expect(markup).toContain('Choose where to save the conversation.');
    expect(markup).toContain('type="submit" appearance="accent" disabled');
  });

  it('renders an inclusive range and a same-conversation re-export choice', () => {
    const markup = String(ConversationExportDialog({ state: {
      source,
      messages,
      draft: {
        scope: { kind: 'range', startMessageId: 'message-2', endMessageId: 'message-3' },
        destination: {
          selectionToken: 'opaque-selection',
          displayPath: '/Users/me/Exports/release-review',
          kind: 'directory',
          existing: { exportId: 'export-1', revision: 4, sourceConversationId: 'conversation-1' },
        },
        writeMode: 'reexport',
        bundle: { ...defaultConversationExportDraft().bundle, includeSummary: true },
      },
    } }));
    expect(markup).toContain('name="conversation-export-scope" value="range" checked');
    expect(markup).toContain('name="conversation-export-start-message"');
    expect(markup).toContain('value="message-2"');
    expect(markup).toContain('/Users/me/Exports/release-review');
    expect(markup).toContain('Revision 4 of this conversation is already there.');
    expect(markup).toContain('name="conversation-export-write-mode" value="reexport" checked');
    expect(markup).toContain('name="conversation-export-write-mode" value="overwrite"');
    expect(markup).toContain('data-bundle-purpose="summary"');
    expect(markup).toContain('Re-export conversation');
    expect(markup).not.toContain('type="submit" appearance="accent" disabled');
  });

  it('requires overwrite for another conversation and explains a non-resumable source', () => {
    const markup = String(ConversationExportDialog({ state: {
      source: { conversationId: 'conversation-1', tool: 'Codex' },
      messages,
      draft: {
        ...defaultConversationExportDraft(),
        destination: {
          selectionToken: 'opaque-selection',
          displayPath: '/Exports/existing',
          kind: 'archive',
          existing: { exportId: 'export-2', revision: 1, sourceConversationId: 'conversation-2' },
        },
      },
    } }));
    expect(markup).toContain('This destination contains a different conversation export.');
    expect(markup).not.toContain('value="reexport"');
    expect(markup).toContain('value="overwrite"');
    expect(markup).toContain('no resumable session ID');
    expect(markup).toContain('Choose re-export or overwrite for the existing bundle.');
  });

  it('disables summary selection and submission when a stale summary choice is unavailable', () => {
    const markup = String(ConversationExportDialog({ state: {
      source,
      messages,
      summaryAvailable: false,
      draft: {
        ...defaultConversationExportDraft(),
        destination: { selectionToken: 'opaque-selection', displayPath: '/Exports/new', kind: 'directory' },
        bundle: { ...defaultConversationExportDraft().bundle, includeSummary: true },
      },
    } }));
    expect(markup).toContain('name="conversation-export-summary" checked disabled');
    expect(markup).toContain('Summary export is unavailable for this conversation.');
    expect(markup).toContain('type="submit" appearance="accent" disabled');
  });
});
