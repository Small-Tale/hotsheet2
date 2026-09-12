import type { ConversationActivity, ConversationMessage } from './ai-conversation';

export const CONVERSATION_EXPORT_MANIFEST_VERSION = 1;

export type ConversationExportScope =
  | { kind: 'all' }
  | { kind: 'range'; startMessageId: string; endMessageId: string };

export type ConversationExportWriteMode = 'create' | 'overwrite' | 'reexport';

export interface ConversationExportBundleOptions {
  includeAttachments: boolean;
  includeMedia: boolean;
  includeSummary: boolean;
}

/** Metadata returned by the trusted destination picker, never constructed from a path input. */
export interface ConversationExportDestination {
  selectionToken: string;
  displayPath: string;
  kind: 'directory' | 'archive';
  existing?: {
    exportId: string;
    revision: number;
    sourceConversationId?: string;
  };
}

export interface ConversationExportPickerRequest {
  suggestedName: string;
  allowedKinds: readonly ConversationExportDestination['kind'][];
  currentSelectionToken?: string;
}

export interface ConversationExportSource {
  conversationId: string;
  tool: string;
  sessionId?: string;
  projectId?: string;
  model?: string;
  effort?: string;
  resumable?: boolean;
}

export interface ConversationExportDraft {
  scope: ConversationExportScope;
  destination?: ConversationExportDestination;
  writeMode: ConversationExportWriteMode;
  bundle: ConversationExportBundleOptions;
}

export interface ConversationExportRequest {
  manifestVersion: typeof CONVERSATION_EXPORT_MANIFEST_VERSION;
  source: ConversationExportSource;
  selectedMessageIds: string[];
  destination: Pick<ConversationExportDestination, 'selectionToken' | 'kind'>;
  writeMode: ConversationExportWriteMode;
  replacesExportId?: string;
  parentRevision?: number;
  bundle: ConversationExportBundleOptions;
  reopen: {
    conversationId: string;
    sessionId?: string;
    tool: string;
    projectId?: string;
    model?: string;
    effort?: string;
    firstMessageId: string;
    lastMessageId: string;
    resumesOriginalSession: boolean;
  };
}

export interface ConversationExportBundleEntry {
  path: string;
  purpose: 'manifest' | 'transcript' | 'structure' | 'summary' | 'attachments' | 'media';
  required: boolean;
}

export interface ConversationExportAsset {
  id: string;
  filename: string;
  mimeType: string;
  kind: 'attachment' | 'media';
  dataBase64: string;
}

export function selectedConversationFileReferences(messages:readonly ConversationMessage[],scope:ConversationExportScope,options:ConversationExportBundleOptions){const files=new Map<string,NonNullable<ConversationMessage['files']>[number]>();for(const message of selectedConversationMessages(messages,scope))for(const file of message.files??[])if((file.kind==='attachment'&&options.includeAttachments)||(file.kind==='media'&&options.includeMedia))files.set(file.id,file);return[...files.values()]}

function bytesToBase64(bytes:Uint8Array){let binary='';for(let offset=0;offset<bytes.length;offset+=0x8000)binary+=String.fromCharCode(...bytes.subarray(offset,offset+0x8000));return btoa(binary)}

export async function conversationExportAssets(messages:readonly ConversationMessage[],draft:ConversationExportDraft,fetchFile:typeof fetch=fetch):Promise<ConversationExportAsset[]>{return Promise.all(selectedConversationFileReferences(messages,draft.scope,draft.bundle).map(async file=>{const response=await fetchFile(file.url);if(!response.ok)throw new Error(`Could not read ${file.filename} for export.`);return{id:file.id,filename:file.filename,mimeType:file.mime_type,kind:file.kind,dataBase64:bytesToBase64(new Uint8Array(await response.arrayBuffer()))}}))}

export interface ConversationExportManifest {
  format: 'hotsheet-conversation-export';
  manifestVersion: typeof CONVERSATION_EXPORT_MANIFEST_VERSION;
  exportId: string;
  revision: number;
  exportedAt: string;
  source: ConversationExportSource;
  selectedMessageIds: string[];
  replacesExportId?: string;
  parentRevision?: number;
  bundle: ConversationExportBundleOptions;
  reopen: ConversationExportRequest['reopen'];
  entries: ConversationExportBundleEntry[];
  assets: Array<Pick<ConversationExportAsset, 'id' | 'filename' | 'mimeType' | 'kind'> & { path: string }>;
}

export interface ConversationExportPayload {
  request: ConversationExportRequest;
  messages: ConversationMessage[];
  activity: ConversationActivity[];
  assets: ConversationExportAsset[];
}

export interface ConversationExportOpenResult {
  displayPath: string;
  manifest: ConversationExportManifest;
  messages: ConversationMessage[];
  activity: ConversationActivity[];
}

export interface ConversationExportWriteResult {
  displayPath: string;
  manifest: ConversationExportManifest;
}

export const DEFAULT_CONVERSATION_EXPORT_BUNDLE: ConversationExportBundleOptions = {
  includeAttachments: true,
  includeMedia: true,
  includeSummary: false,
};

export function defaultConversationExportDraft(): ConversationExportDraft {
  return {
    scope: { kind: 'all' },
    writeMode: 'create',
    bundle: { ...DEFAULT_CONVERSATION_EXPORT_BUNDLE },
  };
}

export function conversationExportMessageLabel(message: ConversationMessage, index: number): string {
  const speaker = message.role === 'user' ? 'You' : 'Assistant';
  const excerpt = message.content.replace(/\s+/g, ' ').trim() || 'Empty message';
  const clipped = excerpt.length > 54 ? `${excerpt.slice(0, 51)}…` : excerpt;
  return `${index + 1}. ${speaker} — ${clipped}`;
}

export function selectedConversationMessages(
  messages: readonly ConversationMessage[],
  scope: ConversationExportScope,
): readonly ConversationMessage[] {
  if (scope.kind === 'all') return messages;
  const startIndex = messages.findIndex(message => message.id === scope.startMessageId);
  const endIndex = messages.findIndex(message => message.id === scope.endMessageId);
  if (startIndex < 0 || endIndex < 0 || startIndex > endIndex) return [];
  return messages.slice(startIndex, endIndex + 1);
}

/** Two-click visual range selection: the first pick anchors, the second completes an ordered range, and a third starts over. */
export function conversationExportScopeAfterMessagePick(
  messages: readonly ConversationMessage[],
  scope: ConversationExportScope,
  messageId: string,
): ConversationExportScope {
  const picked = messages.findIndex(message => message.id === messageId);
  if (picked < 0) return scope;
  if (scope.kind === 'all') return { kind: 'range', startMessageId: messageId, endMessageId: messageId };
  const start = messages.findIndex(message => message.id === scope.startMessageId);
  const end = messages.findIndex(message => message.id === scope.endMessageId);
  if (start < 0 || end < 0 || start !== end || picked === start) return { kind: 'range', startMessageId: messageId, endMessageId: messageId };
  return {
    kind: 'range',
    startMessageId: messages[Math.min(start, picked)].id,
    endMessageId: messages[Math.max(start, picked)].id,
  };
}

export function conversationExportValidation(
  messages: readonly ConversationMessage[],
  draft: ConversationExportDraft,
  source?: ConversationExportSource,
): string | undefined {
  const selected = selectedConversationMessages(messages, draft.scope);
  if (selected.length === 0) return messages.length === 0 ? 'There are no messages to save.' : 'Choose a valid message range.';
  if (!draft.destination?.selectionToken) return 'Choose where to save the conversation.';
  if (draft.writeMode === 'create' && draft.destination.existing) return 'Choose re-export or overwrite for the existing bundle.';
  if (draft.writeMode !== 'create' && !draft.destination.existing) return 'The selected destination does not contain an export to replace.';
  if (draft.writeMode === 'reexport' && draft.destination.existing?.sourceConversationId !== source?.conversationId) {
    return 'Re-export is only available for an earlier export of this conversation.';
  }
  return undefined;
}

/** The stable logical contents of a bundle; attachment/media files are expanded by the server. */
export function conversationExportBundleEntries(options: ConversationExportBundleOptions): ConversationExportBundleEntry[] {
  const entries: ConversationExportBundleEntry[] = [
    { path: 'manifest.json', purpose: 'manifest', required: true },
    { path: 'transcript.md', purpose: 'transcript', required: true },
    { path: 'conversation.json', purpose: 'structure', required: true },
  ];
  if (options.includeSummary) entries.push({ path: 'summary.md', purpose: 'summary', required: false });
  if (options.includeAttachments) entries.push({ path: 'attachments/', purpose: 'attachments', required: false });
  if (options.includeMedia) entries.push({ path: 'media/', purpose: 'media', required: false });
  return entries;
}

export function buildConversationExportRequest(
  source: ConversationExportSource,
  messages: readonly ConversationMessage[],
  draft: ConversationExportDraft,
): ConversationExportRequest {
  const validation = conversationExportValidation(messages, draft, source);
  if (validation) throw new Error(validation);
  const selected = selectedConversationMessages(messages, draft.scope);
  const destination = draft.destination!;
  const existing = destination.existing;
  const lastSourceMessageId = messages.at(-1)?.id;
  return {
    manifestVersion: CONVERSATION_EXPORT_MANIFEST_VERSION,
    source: { ...source },
    selectedMessageIds: selected.map(message => message.id),
    destination: { selectionToken: destination.selectionToken, kind: destination.kind },
    writeMode: draft.writeMode,
    replacesExportId: existing?.exportId,
    parentRevision: draft.writeMode === 'reexport' ? existing?.revision : undefined,
    bundle: { ...draft.bundle },
    reopen: {
      ...source,
      firstMessageId: selected[0].id,
      lastMessageId: selected.at(-1)!.id,
      resumesOriginalSession: Boolean(source.sessionId && source.resumable !== false && selected.at(-1)?.id === lastSourceMessageId),
    },
  };
}

export function suggestedConversationExportName(tool: string, now = new Date()): string {
  const safeTool = tool.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'conversation';
  return `${safeTool}-conversation-${now.toISOString().slice(0, 10)}`;
}

function quotedBlock(value: string): string {
  return value.trim().split('\n').map(line => `> ${line}`).join('\n');
}

/** Human-readable companion to the lossless structured transcript. */
export function conversationTranscriptMarkdown(
  source: ConversationExportSource,
  messages: readonly ConversationMessage[],
  activity: readonly ConversationActivity[] = [],
): string {
  const lines = [`# ${source.tool} conversation`, ''];
  for (const message of messages) {
    lines.push(`## ${message.role === 'user' ? 'You' : 'Assistant'}`, '', message.content.trim() || '_Empty message_', '');
    if (message.status && message.status !== 'completed') lines.push(`_Status: ${message.status}_`, '');
    if (message.usage) lines.push(`_Usage: ${message.usage.tokensIn} input tokens, ${message.usage.tokensOut} output tokens${message.usage.costUsd === undefined ? '' : `, $${message.usage.costUsd.toFixed(4)}`}_`, '');
  }
  if (activity.length) {
    lines.push('## Activity', '');
    for (const item of activity) lines.push(`- **${item.tool} · ${item.kind}:** ${item.summary}`);
    lines.push('');
  }
  return `${lines.join('\n').trim()}\n`;
}

/** Deterministic, local overview: useful for quick re-absorption without claiming AI authorship. */
export function conversationSummaryMarkdown(
  source: ConversationExportSource,
  messages: readonly ConversationMessage[],
): string {
  const firstUser = messages.find(message => message.role === 'user');
  const lastAssistant = [...messages].reverse().find(message => message.role === 'assistant');
  const lines = [
    `# ${source.tool} conversation summary`,
    '',
    `Selected transcript: ${messages.length} message${messages.length === 1 ? '' : 's'}.`,
  ];
  if (firstUser) lines.push('', '## Opening request', '', quotedBlock(firstUser.content.slice(0, 1_200)));
  if (lastAssistant) lines.push('', '## Latest outcome', '', quotedBlock(lastAssistant.content.slice(0, 2_400)));
  const topics = messages.filter(message => message.role === 'user').slice(1).map(message => message.content.replace(/\s+/g, ' ').trim()).filter(Boolean).slice(0, 8);
  if (topics.length) lines.push('', '## Follow-up requests', '', ...topics.map(topic => `- ${topic.slice(0, 240)}`));
  return `${lines.join('\n').trim()}\n`;
}
