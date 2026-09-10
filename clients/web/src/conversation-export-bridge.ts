import { randomUUID } from 'node:crypto';
import { mkdir, readFile, rename, rm, stat, writeFile } from 'node:fs/promises';
import { basename, join, resolve } from 'node:path';

import {
  CONVERSATION_EXPORT_MANIFEST_VERSION,
  conversationExportBundleEntries,
  type ConversationExportDestination,
  type ConversationExportManifest,
  type ConversationExportOpenResult,
  type ConversationExportPayload,
  type ConversationExportWriteResult,
  conversationSummaryMarkdown,
  conversationTranscriptMarkdown,
} from './conversation-export';

const FORMAT = 'hotsheet-conversation-export';
const BUNDLE_SUFFIX = '.hotsheet-chat';
const MAX_ASSET_BYTES = 50 * 1024 * 1024;
const MAX_TOTAL_ASSET_BYTES = 200 * 1024 * 1024;

type FolderChooser = () => Promise<string | undefined>;

function errorMessage(reason: unknown): string { return reason instanceof Error ? reason.message : String(reason); }
async function exists(path: string): Promise<boolean> { try { await stat(path); return true; } catch { return false; } }

function safeSuggestedName(value: string): string {
  const clean = value.trim().replace(/[^A-Za-z0-9._ -]+/g, '-').replace(/^\.+|\.+$/g, '').slice(0, 120);
  if (!clean) throw new Error('The conversation export needs a valid suggested name.');
  return clean.endsWith(BUNDLE_SUFFIX) ? clean : `${clean}${BUNDLE_SUFFIX}`;
}

function safeAssetName(id: string, filename: string): string {
  const leaf = basename(filename).replace(/[^A-Za-z0-9._ -]+/g, '-').slice(-160) || 'asset';
  const safeId = id.replace(/[^A-Za-z0-9_-]+/g, '-').slice(0, 48) || 'asset';
  return `${safeId}-${leaf}`;
}

function isManifest(value: unknown): value is ConversationExportManifest {
  if (!value || typeof value !== 'object') return false;
  const manifest = value as Partial<ConversationExportManifest>;
  return manifest.format === FORMAT && manifest.manifestVersion === CONVERSATION_EXPORT_MANIFEST_VERSION && typeof manifest.exportId === 'string' && typeof manifest.revision === 'number' && Number.isInteger(manifest.revision) && manifest.revision > 0 && Array.isArray(manifest.selectedMessageIds) && Boolean(manifest.source?.conversationId) && Boolean(manifest.reopen?.conversationId);
}

async function readManifest(path: string): Promise<ConversationExportManifest | undefined> {
  try { const value: unknown = JSON.parse(await readFile(join(path, 'manifest.json'), 'utf8')); return isManifest(value) ? value : undefined; } catch { return undefined; }
}

async function atomicText(path: string, content: string): Promise<void> {
  const temporary = `${path}.${randomUUID()}.tmp`;
  await writeFile(temporary, content, 'utf8');
  await rename(temporary, path);
}

function validatePayload(payload: ConversationExportPayload): void {
  const { request, messages, activity, assets } = payload;
  if ((request as { manifestVersion?: unknown }).manifestVersion !== CONVERSATION_EXPORT_MANIFEST_VERSION) throw new Error('This conversation export version is not supported.');
  if (request.destination.kind !== 'directory') throw new Error('Only folder conversation bundles are supported.');
  if (!Array.isArray(messages) || messages.length === 0) throw new Error('The conversation export contains no messages.');
  if ((messages as unknown[]).some(message => !message || typeof message !== 'object' || typeof (message as { id?: unknown }).id !== 'string' || !['user', 'assistant'].includes(String((message as { role?: unknown }).role)) || typeof (message as { content?: unknown }).content !== 'string')) throw new Error('The conversation export contains an invalid message.');
  if (messages.map(message => message.id).join('\0') !== request.selectedMessageIds.join('\0')) throw new Error('The selected message IDs do not match the exported transcript.');
  if (!Array.isArray(activity) || !Array.isArray(assets)) throw new Error('The conversation export payload is incomplete.');
  let total = 0;
  for (const asset of assets) {
    if (([asset] as unknown[]).some(value => !value || typeof value !== 'object' || typeof (value as { id?: unknown }).id !== 'string' || typeof (value as { filename?: unknown }).filename !== 'string' || typeof (value as { mimeType?: unknown }).mimeType !== 'string' || !['attachment', 'media'].includes(String((value as { kind?: unknown }).kind)) || typeof (value as { dataBase64?: unknown }).dataBase64 !== 'string')) throw new Error('The conversation export contains an invalid asset.');
    const bytes = Buffer.from(asset.dataBase64, 'base64').byteLength;
    if (bytes > MAX_ASSET_BYTES) throw new Error(`${asset.filename} exceeds the 50 MB conversation-export limit.`);
    total += bytes;
  }
  if (total > MAX_TOTAL_ASSET_BYTES) throw new Error('Conversation export assets exceed the 200 MB bundle limit.');
}

export interface ConversationExportBridge {
  chooseDestination(suggestedName: string): Promise<ConversationExportDestination | undefined>;
  write(payload: ConversationExportPayload): Promise<ConversationExportWriteResult>;
  open(): Promise<ConversationExportOpenResult | undefined>;
}

export function createConversationExportBridge(
  chooseFolder: FolderChooser,
  options: { now?: () => Date; id?: () => string } = {},
): ConversationExportBridge {
  const selections = new Map<string, string>();
  const now = options.now ?? (() => new Date());
  const id = options.id ?? randomUUID;
  return {
    async chooseDestination(suggestedName) {
      const parent = await chooseFolder();
      if (!parent) return undefined;
      const path = resolve(parent, safeSuggestedName(suggestedName));
      const token = id();
      selections.set(token, path);
      const existing = await readManifest(path);
      return {
        selectionToken: token,
        displayPath: path,
        kind: 'directory',
        ...(existing ? { existing: { exportId: existing.exportId, revision: existing.revision, sourceConversationId: existing.source.conversationId } } : {}),
      };
    },

    async write(payload) {
      validatePayload(payload);
      const { request, messages, activity, assets } = payload;
      const path = selections.get(request.destination.selectionToken);
      if (!path) throw new Error('The conversation export destination expired. Choose it again.');
      const occupied = await exists(path), existing = occupied ? await readManifest(path) : undefined;
      if (request.writeMode === 'create' && occupied) throw new Error('That export bundle already exists. Choose re-export, overwrite, or another destination.');
      if (request.writeMode !== 'create' && !existing) throw new Error('The selected folder is not a valid Hot Sheet conversation export.');
      if (request.replacesExportId && existing?.exportId !== request.replacesExportId) throw new Error('The export changed after it was selected. Choose the destination again.');
      if (request.writeMode === 'reexport' && (existing?.source.conversationId !== request.source.conversationId || existing.revision !== request.parentRevision)) throw new Error('The previous conversation export revision no longer matches. Choose it again.');
      if (!occupied) await mkdir(path);
      const exportedAssets: ConversationExportManifest['assets'] = [];
      for (const directory of ['attachments', 'media']) await rm(join(path, directory), { recursive: true, force: true });
      for (const asset of assets) {
        if (asset.kind === 'attachment' && !request.bundle.includeAttachments) continue;
        if (asset.kind === 'media' && !request.bundle.includeMedia) continue;
        const directory = asset.kind === 'media' ? 'media' : 'attachments', filename = safeAssetName(asset.id, asset.filename), relativePath = `${directory}/${filename}`;
        await mkdir(join(path, directory), { recursive: true });
        await writeFile(join(path, relativePath), Buffer.from(asset.dataBase64, 'base64'));
        exportedAssets.push({ id: asset.id, filename: asset.filename, mimeType: asset.mimeType, kind: asset.kind, path: relativePath });
      }
      if (request.bundle.includeAttachments) await mkdir(join(path, 'attachments'), { recursive: true });
      if (request.bundle.includeMedia) await mkdir(join(path, 'media'), { recursive: true });
      const revision = request.writeMode === 'reexport' ? existing!.revision + 1 : 1;
      const exportId = request.writeMode === 'reexport' ? existing!.exportId : id();
      const manifest: ConversationExportManifest = {
        format: FORMAT,
        manifestVersion: CONVERSATION_EXPORT_MANIFEST_VERSION,
        exportId,
        revision,
        exportedAt: now().toISOString(),
        source: request.source,
        selectedMessageIds: request.selectedMessageIds,
        ...(request.replacesExportId ? { replacesExportId: request.replacesExportId } : {}),
        ...(request.parentRevision === undefined ? {} : { parentRevision: request.parentRevision }),
        bundle: request.bundle,
        reopen: request.reopen,
        entries: conversationExportBundleEntries(request.bundle),
        assets: exportedAssets,
      };
      await atomicText(join(path, 'transcript.md'), conversationTranscriptMarkdown(request.source, messages, activity));
      await atomicText(join(path, 'conversation.json'), `${JSON.stringify({ messages, activity }, null, 2)}\n`);
      if (request.bundle.includeSummary) await atomicText(join(path, 'summary.md'), conversationSummaryMarkdown(request.source, messages));
      else await rm(join(path, 'summary.md'), { force: true });
      await atomicText(join(path, 'manifest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
      return { displayPath: path, manifest };
    },

    async open() {
      const chosen = await chooseFolder();
      if (!chosen) return undefined;
      const path = resolve(chosen), manifest = await readManifest(path);
      if (!manifest) throw new Error('Choose a valid .hotsheet-chat bundle containing manifest.json.');
      let structured: unknown;
      try { structured = JSON.parse(await readFile(join(path, 'conversation.json'), 'utf8')); } catch (reason) { throw new Error(`The saved conversation transcript could not be read: ${errorMessage(reason)}`, { cause: reason }); }
      const value = structured as { messages?: ConversationExportOpenResult['messages']; activity?: ConversationExportOpenResult['activity'] };
      const messages = value.messages ?? [], activity = value.activity ?? [];
      validatePayload({ request: { manifestVersion: manifest.manifestVersion, source: manifest.source, selectedMessageIds: manifest.selectedMessageIds, destination: { selectionToken: 'open', kind: 'directory' }, writeMode: 'create', bundle: manifest.bundle, reopen: manifest.reopen }, messages, activity, assets: [] });
      return { displayPath: path, manifest, messages, activity };
    },
  };
}
