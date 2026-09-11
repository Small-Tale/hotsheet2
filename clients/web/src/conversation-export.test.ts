import { describe, expect, it } from 'vitest';

import type { ConversationMessage } from './ai-conversation';
import {
  buildConversationExportRequest,
  conversationExportBundleEntries,
  conversationExportScopeAfterMessagePick,
  conversationExportValidation,
  conversationSummaryMarkdown,
  conversationTranscriptMarkdown,
  defaultConversationExportDraft,
  selectedConversationMessages,
  suggestedConversationExportName,
} from './conversation-export';

const messages: ConversationMessage[] = [
  { id: 'message-1', role: 'user', content: 'Plan the release.' },
  { id: 'message-2', role: 'assistant', content: 'I will inspect the release checks.' },
  { id: 'message-3', role: 'user', content: 'Only include verified changes.' },
  { id: 'message-4', role: 'assistant', content: 'The verified changes are ready.' },
];

describe('conversation export contract', () => {
  it('selects an inclusive ordered range and rejects reversed or missing boundaries', () => {
    expect(selectedConversationMessages(messages, { kind: 'range', startMessageId: 'message-2', endMessageId: 'message-3' }).map(message => message.id)).toEqual(['message-2', 'message-3']);
    expect(selectedConversationMessages(messages, { kind: 'range', startMessageId: 'message-3', endMessageId: 'message-2' })).toEqual([]);
    expect(selectedConversationMessages(messages, { kind: 'range', startMessageId: 'missing', endMessageId: 'message-4' })).toEqual([]);
  });

  it('builds a visual range with two message picks and starts over on the third',()=>{
    const first=conversationExportScopeAfterMessagePick(messages,{kind:'all'},'message-3');
    expect(first).toEqual({kind:'range',startMessageId:'message-3',endMessageId:'message-3'});
    const second=conversationExportScopeAfterMessagePick(messages,first,'message-1');
    expect(second).toEqual({kind:'range',startMessageId:'message-1',endMessageId:'message-3'});
    expect(conversationExportScopeAfterMessagePick(messages,second,'message-2')).toEqual({kind:'range',startMessageId:'message-2',endMessageId:'message-2'});
    expect(conversationExportScopeAfterMessagePick(messages,second,'missing')).toEqual(second);
  });

  it('describes a portable bundle without inventing individual attachment paths', () => {
    expect(conversationExportBundleEntries({
      includeAttachments: true,
      includeMedia: true,
      includeSummary: true,
    })).toEqual([
      { path: 'manifest.json', purpose: 'manifest', required: true },
      { path: 'transcript.md', purpose: 'transcript', required: true },
      { path: 'conversation.json', purpose: 'structure', required: true },
      { path: 'summary.md', purpose: 'summary', required: false },
      { path: 'attachments/', purpose: 'attachments', required: false },
      { path: 'media/', purpose: 'media', required: false },
    ]);
  });

  it('builds re-export lineage and only marks a complete tail selection resumable', () => {
    const request = buildConversationExportRequest(
      { conversationId: 'conversation-1', sessionId: 'session-1', tool: 'Codex', projectId: 'project-1', model: 'gpt-5', effort: 'high' },
      messages,
      {
        scope: { kind: 'range', startMessageId: 'message-2', endMessageId: 'message-4' },
        destination: {
          selectionToken: 'opaque-selection',
          displayPath: '/Exports/release-review',
          kind: 'directory',
          existing: { exportId: 'export-1', revision: 3, sourceConversationId: 'conversation-1' },
        },
        writeMode: 'reexport',
        bundle: { ...defaultConversationExportDraft().bundle, includeSummary: true },
      },
    );
    expect(request).toMatchObject({
      manifestVersion: 1,
      selectedMessageIds: ['message-2', 'message-3', 'message-4'],
      destination: { selectionToken: 'opaque-selection', kind: 'directory' },
      writeMode: 'reexport',
      replacesExportId: 'export-1',
      parentRevision: 3,
      bundle: { includeSummary: true },
      reopen: {
        conversationId: 'conversation-1',
        sessionId: 'session-1',
        firstMessageId: 'message-2',
        lastMessageId: 'message-4',
        resumesOriginalSession: true,
      },
    });
    expect(buildConversationExportRequest(
      { conversationId: 'conversation-1', sessionId: 'session-1', tool: 'Codex' },
      messages,
      {
        ...defaultConversationExportDraft(),
        scope: { kind: 'range', startMessageId: 'message-1', endMessageId: 'message-2' },
        destination: { selectionToken: 'new-selection', displayPath: '/Exports/partial', kind: 'directory' },
      },
    ).reopen.resumesOriginalSession).toBe(false);
  });

  it('requires an explicit collision choice and prevents cross-conversation re-export', () => {
    const destination = {
      selectionToken: 'opaque-selection',
      displayPath: '/Exports/existing',
      kind: 'directory' as const,
      existing: { exportId: 'export-other', revision: 2, sourceConversationId: 'conversation-other' },
    };
    const draft = { ...defaultConversationExportDraft(), destination };
    expect(conversationExportValidation(messages, draft, { conversationId: 'conversation-1', tool: 'Codex' })).toBe('Choose re-export or overwrite for the existing bundle.');
    expect(conversationExportValidation(messages, { ...draft, writeMode: 'reexport' }, { conversationId: 'conversation-1', tool: 'Codex' })).toBe('Re-export is only available for an earlier export of this conversation.');
    expect(conversationExportValidation(messages, { ...draft, writeMode: 'overwrite' }, { conversationId: 'conversation-1', tool: 'Codex' })).toBeUndefined();
  });

  it('supplies safe defaults and a stable picker suggestion', () => {
    expect(defaultConversationExportDraft()).toEqual({
      scope: { kind: 'all' },
      writeMode: 'create',
      bundle: {
        includeAttachments: true,
        includeMedia: true,
        includeSummary: false,
      },
    });
    expect(suggestedConversationExportName('Claude Code', new Date('2026-09-10T05:00:00Z'))).toBe('claude-code-conversation-2026-09-10');
  });

  it('renders lossless readable Markdown and an explicitly local concise summary', () => {
    const source = { conversationId: 'conversation-1', tool: 'Codex' };
    expect(conversationTranscriptMarkdown(source, messages)).toContain('## You\n\nPlan the release.');
    expect(conversationTranscriptMarkdown(source, messages)).toContain('## Assistant\n\nThe verified changes are ready.');
    const summary = conversationSummaryMarkdown(source, messages);
    expect(summary).toContain('# Codex conversation summary');
    expect(summary).toContain('## Opening request');
    expect(summary).toContain('## Latest outcome');
    expect(summary).toContain('Only include verified changes.');
  });
});
