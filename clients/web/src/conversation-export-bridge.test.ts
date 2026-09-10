import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import type { ConversationMessage } from './ai-conversation';
import { buildConversationExportRequest, defaultConversationExportDraft } from './conversation-export';
import { createConversationExportBridge } from './conversation-export-bridge';

const cleanup: string[] = [];
afterEach(async () => { await Promise.all(cleanup.splice(0).map(path => rm(path, { recursive: true, force: true }))); });

const source = { conversationId: 'chat-1', sessionId: 'session-1', tool: 'codex', projectId: 'project-1', model: 'gpt-test', effort: 'high' };
const messages: ConversationMessage[] = [
  { id: 'one', role: 'user', content: 'Review the release notes.' },
  { id: 'two', role: 'assistant', content: 'The release notes are ready.', status: 'completed' },
];

describe('conversation export bridge', () => {
  it('writes and reopens a portable folder bundle with readable, structured, summary, and asset content', async () => {
    const parent = await mkdtemp(join(tmpdir(), 'hotsheet-chat-export-'));cleanup.push(parent);
    const bundle = join(parent, 'codex-conversation-2026-09-10.hotsheet-chat');
    const choices = [parent, bundle];
    const bridge = createConversationExportBridge(async () => choices.shift(), { now: () => new Date('2026-09-10T12:00:00Z'), id: (() => { let value = 0; return () => `id-${++value}`; })() });
    const destination = await bridge.chooseDestination('codex-conversation-2026-09-10');
    const draft = { ...defaultConversationExportDraft(), destination, bundle: { ...defaultConversationExportDraft().bundle, includeSummary: true } };
    const request = buildConversationExportRequest(source, messages, draft);
    const written = await bridge.write({ request, messages, activity: [{ id: 'activity-1', tool: 'codex', kind: 'test', summary: 'Ran release checks', importance: 'normal' }], assets: [{ id: 'proof', filename: 'proof.txt', mimeType: 'text/plain', kind: 'attachment', dataBase64: Buffer.from('verified').toString('base64') }] });
    expect(written.manifest).toMatchObject({ exportId: 'id-2', revision: 1, selectedMessageIds: ['one', 'two'], reopen: { resumesOriginalSession: true } });
    await expect(readFile(join(bundle, 'transcript.md'), 'utf8')).resolves.toContain('## Assistant\n\nThe release notes are ready.');
    await expect(readFile(join(bundle, 'summary.md'), 'utf8')).resolves.toContain('## Latest outcome');
    await expect(readFile(join(bundle, 'attachments/proof-proof.txt'), 'utf8')).resolves.toBe('verified');
    const reopened = await bridge.open();
    expect(reopened).toMatchObject({ displayPath: bundle, manifest: { exportId: 'id-2' }, messages, activity: [{ id: 'activity-1' }] });
  });

  it('detects an existing export and advances same-conversation re-export lineage', async () => {
    const parent = await mkdtemp(join(tmpdir(), 'hotsheet-chat-reexport-'));cleanup.push(parent);
    const ids = (() => { let value = 0; return () => `id-${++value}`; })();
    const bridge = createConversationExportBridge(async () => parent, { id: ids });
    const first = await bridge.chooseDestination('saved-chat');
    await bridge.write({ request: buildConversationExportRequest(source, messages, { ...defaultConversationExportDraft(), destination: first }), messages, activity: [], assets: [] });
    const existing = await bridge.chooseDestination('saved-chat');
    expect(existing?.existing).toEqual({ exportId: 'id-2', revision: 1, sourceConversationId: 'chat-1' });
    const request = buildConversationExportRequest(source, messages, { ...defaultConversationExportDraft(), destination: existing, writeMode: 'reexport' });
    const updated = await bridge.write({ request, messages, activity: [], assets: [] });
    expect(updated.manifest).toMatchObject({ exportId: 'id-2', revision: 2, parentRevision: 1, replacesExportId: 'id-2' });
  });

  it('rejects expired destination tokens and invalid reopen folders', async () => {
    const parent = await mkdtemp(join(tmpdir(), 'hotsheet-chat-invalid-'));cleanup.push(parent);
    const bridge = createConversationExportBridge(async () => parent);
    const destination = { selectionToken: 'expired', displayPath: parent, kind: 'directory' as const };
    const request = buildConversationExportRequest(source, messages, { ...defaultConversationExportDraft(), destination });
    await expect(bridge.write({ request, messages, activity: [], assets: [] })).rejects.toThrow('destination expired');
    await expect(bridge.open()).rejects.toThrow('valid .hotsheet-chat bundle');
  });
});
