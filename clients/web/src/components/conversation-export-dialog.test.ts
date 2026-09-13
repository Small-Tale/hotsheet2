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
  it('skips message scope when the chat has no selection', () => {
    const markup = String(ConversationExportDialog({ state: { source, messages, draft: defaultConversationExportDraft(),step:2 } }));
    expect(markup).toContain('data-component="conversation-export-dialog"');
    expect(markup).toContain('data-step="2"');
    expect(markup).toContain('data-navigation="none"');
    expect(markup).toContain('data-active-side="b"');
    expect(markup).toContain('data-transition-style="none"');
    expect(markup).toContain('Save conversation');
    expect(markup).toContain('Bundle contents');
    expect(markup).toContain('data-action="submit-conversation-export"');
    expect(markup).not.toContain('previous-conversation-export-step');
  });

  it('offers the already-selected chat range without asking users to pick it again',()=>{
    const selectedRange={kind:'range' as const,startMessageId:'message-2',endMessageId:'message-3'};
    const markup=String(ConversationExportDialog({state:{source,messages,draft:{...defaultConversationExportDraft(),scope:selectedRange},selectedRange,step:1}}));
    expect(markup).toContain('data-step="1"');
    expect(markup).toContain('data-active-side="a"');
    expect(markup).toContain('Step 1 of 2');
    expect(markup).toContain('Choose scope');
    expect(markup).toContain('name="conversation-export-scope" value="all"');
    expect(markup).not.toContain('name="conversation-export-scope" value="all" checked');
    expect(markup).toContain('name="conversation-export-scope" value="range" checked');
    expect(markup).toContain('2 messages selected in the chat.');
    expect(markup).not.toContain('class="ai-conversation__message');
    expect(markup).not.toContain('pick-conversation-export-message');
    expect(markup).toContain('data-action="next-conversation-export-step"');
  });

  it('pushes forward and pops backward with the shared in-content back affordance',()=>{
    const selectedRange={kind:'range' as const,startMessageId:'message-2',endMessageId:'message-3'};
    const forward=String(ConversationExportDialog({state:{source,messages,draft:{...defaultConversationExportDraft(),scope:selectedRange},selectedRange,step:2,navigation:'push'}}));
    expect(forward).toContain('data-navigation="push"');
    expect(forward).toContain('data-transition-style="push"');
    expect(forward).toContain('data-transition-direction="forward"');
    expect(forward).toContain('class="flow-back-button"');
    expect(forward).toContain('data-action="previous-conversation-export-step"');
    expect(forward).toContain('data-lucide="chevron-left"');
    expect(forward).toContain('Message scope');
    const backward=String(ConversationExportDialog({state:{source,messages,draft:{...defaultConversationExportDraft(),scope:selectedRange},selectedRange,step:1,navigation:'pop'}}));
    expect(backward).toContain('data-navigation="pop"');
    expect(backward).toContain('data-transition-direction="backward"');
  });

  it('reviews an inclusive range and a same-conversation re-export choice', () => {
    const selectedRange={kind:'range' as const,startMessageId:'message-2',endMessageId:'message-3'};
    const markup = String(ConversationExportDialog({ state: {
      source,
      messages,
      draft: {
        scope: selectedRange,
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
      selectedRange,
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
    expect(markup).not.toContain('Step 2 of 2');
    expect(markup).toContain('Save conversation');
    expect(markup).not.toContain('Choose where to save the conversation.');
    expect(markup).not.toContain('disabled>Save conversation');
    expect(markup).not.toContain('manifest.json');
  });
});
