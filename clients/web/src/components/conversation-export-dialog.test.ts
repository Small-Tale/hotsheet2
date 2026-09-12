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
  it('starts with a compact visual message-selection step', () => {
    const markup = String(ConversationExportDialog({ state: { source, messages, draft: defaultConversationExportDraft() } }));
    expect(markup).toContain('data-component="conversation-export-dialog"');
    expect(markup).toContain('data-step="1"');
    expect(markup).toContain('Step 1 of 2');
    expect(markup).toContain('Choose messages');
    expect(markup).toContain('data-action="submit-conversation-export"');
    expect(markup).toContain('name="conversation-export-scope" value="all" checked');
    expect(markup).toContain('name="conversation-export-scope" value="range"');
    expect(markup.match(/class="ai-conversation__message/g)).toHaveLength(3);
    expect(markup).toContain('class="markdown-preview"');
    expect(markup).toContain('Plan the release.');
    expect(markup).toContain('3 of 3 messages selected');
    expect(markup).toContain('data-action="next-conversation-export-step"');
    expect(markup).not.toContain('No destination selected');
    expect(markup).not.toContain('Reopen metadata is always included.');
  });

  it('reviews an inclusive range and a same-conversation re-export choice', () => {
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
      step:2,
    } }));
    expect(markup).toContain('Step 2 of 2');
    expect(markup).toContain('Revision 4 of this conversation is already there.');
    expect(markup).toContain('name="conversation-export-write-mode" value="reexport" checked');
    expect(markup).toContain('name="conversation-export-write-mode" value="overwrite"');
    expect(markup).toContain('Re-export conversation');
    expect(markup).not.toContain('data-action="finish-conversation-export" disabled');
  });

  it('requires overwrite for another conversation and explains a non-resumable source', () => {
    const markup = String(ConversationExportDialog({ state: {
      source: { conversationId: 'conversation-1', tool: 'Codex' },
      messages,
      step:2,
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
    expect(markup).toContain('Choose re-export or overwrite for the existing bundle.');
  });

  it('disables summary selection and submission when a stale summary choice is unavailable', () => {
    const markup = String(ConversationExportDialog({ state: {
      source,
      messages,
      step:2,
      summaryAvailable: false,
      draft: {
        ...defaultConversationExportDraft(),
        destination: { selectionToken: 'opaque-selection', displayPath: '/Exports/new', kind: 'directory' },
        bundle: { ...defaultConversationExportDraft().bundle, includeSummary: true },
      },
    } }));
    expect(markup).toContain('name="conversation-export-summary" checked disabled');
    expect(markup).toContain('Summary export is unavailable for this conversation.');
  });

  it('defers destination choice to the final save action',()=>{
    const markup=String(ConversationExportDialog({state:{source,messages,draft:defaultConversationExportDraft(),step:2}}));
    expect(markup).toContain('Step 2 of 2');
    expect(markup).toContain('Save conversation');
    expect(markup).not.toContain('Choose where to save the conversation.');
    expect(markup).not.toContain('disabled>Save conversation');
    expect(markup).not.toContain('manifest.json');
  });
});
