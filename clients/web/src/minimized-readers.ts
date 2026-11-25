import type { FullTicket, Note } from './api';
import type { InspectorTab } from './components/ticket-inspector';
import { DETAILS_FEEDBACK_ID } from './feedback-needed';
import type { InlineFeedbackReply } from './feedback-replies';
import { isRecord, tryParseJson } from './json-value';
import { loadMinimizedRecords, saveMinimizedRecords } from './minimized-dialogs';
import { type TicketReaderEditState, ticketReaderEditState, type TicketReaderFrame } from './ticket-reader-stack';

/**
 * Minimized ticket readers (HS2-Q8WF64). Minimizing a reader keeps its frame and every draft but
 * takes it off the modal stack, so the workspace, terminals, and other dialogs stay usable. Each
 * minimized reader belongs to the project tab it was minimized from, and the list survives a reload:
 * the drafts a reader keeps outside the ticket draft store (the new-note composer, inline feedback
 * replies, choice selections, and a response in progress) are kept with its record here, and its
 * unsaved field edits stay in the ticket draft store (`ticket-draft-store.ts`).
 */

/** The response a minimized reader was writing to a feedback request (HS2-VBFSZJ purpose). */
export interface MinimizedReaderResponse {
  request: Pick<Note, 'id' | 'kind' | 'text' | 'created_at' | 'edited_at'>;
  draft: string;
}

/** The drafts a minimized reader holds that would be lost if it closed. */
export interface MinimizedReaderDrafts {
  composingNote: boolean;
  newNoteDraft: string;
  inlineFeedbackReplies: Record<string, InlineFeedbackReply[]>;
  feedbackChoiceSelections: Record<string, string[]>;
  response?: MinimizedReaderResponse;
  /** Field edits a minimize-time save could not commit; their text is in the ticket draft store. */
  unsavedFields: MinimizedReaderField[];
}

export type MinimizedReaderField = 'details' | 'note' | 'blocked_reason';

export interface MinimizedReader {
  /** The reader frame id; a restored reader keeps it, so its drafts stay addressed. */
  id: string;
  /** The project tab the reader was minimized from; only that tab lists it. */
  tabProjectId: string;
  /** The project that owns the ticket (a linked reader may show another project's ticket). */
  projectId: string;
  qualifiedId: string;
  slug: string;
  title: string;
  category: string;
  activeTab: InspectorTab;
  minimizedAt: number;
  drafts: MinimizedReaderDrafts;
  /** The live frame, present until a reload; a reloaded record rebuilds it from the ticket. */
  frame?: TicketReaderFrame;
}

export const MINIMIZED_READERS_STORAGE_KEY = 'hotsheet.minimized-readers.v1';

/** The labels a minimized ticket's draft indicator shows, in the order one is chosen. */
export const MINIMIZED_DRAFT_LABELS = {
  unsavedEdit: 'Unsaved edit',
  unsentNote: 'Unsent note',
  reply: 'Reply in progress',
  /** A minimized new-ticket composer holding a title, details, or staged files (HS2-3JR0ZH). */
  newTicket: 'Unsaved ticket',
} as const;

export type MinimizedDraftLabel = (typeof MINIMIZED_DRAFT_LABELS)[keyof typeof MINIMIZED_DRAFT_LABELS];

const hasText = (value: string) => value.trim().length > 0;

/** The drafts a frame's edit state holds, plus a response in progress from the response registry. */
export function minimizedReaderDrafts(
  edit: TicketReaderEditState,
  response?: MinimizedReaderResponse,
): MinimizedReaderDrafts {
  const unsavedFields: MinimizedReaderField[] = [];
  if (edit.detailsMode === 'write' && edit.detailsDraft !== edit.detailsBase) unsavedFields.push('details');
  if (edit.editingNoteId && !response && edit.noteDraft !== edit.noteBase) unsavedFields.push('note');
  if (edit.blockedReasonEditing && edit.blockedReasonDraft.trim() !== edit.blockedReasonBase.trim())
    unsavedFields.push('blocked_reason');
  return {
    composingNote: edit.composingNote,
    newNoteDraft: edit.composingNote ? edit.newNoteDraft : '',
    inlineFeedbackReplies: Object.fromEntries(
      Object.entries(edit.inlineFeedbackReplies).filter(([, replies]) => replies.length > 0),
    ),
    feedbackChoiceSelections: Object.fromEntries(
      Object.entries(edit.feedbackChoiceSelections).filter(([, choices]) => choices.length > 0),
    ),
    ...(response ? { response } : {}),
    unsavedFields,
  };
}

/** The one draft indicator a minimized ticket shows, or `undefined` when closing it loses nothing. */
export function minimizedDraftLabel(drafts: MinimizedReaderDrafts): MinimizedDraftLabel | undefined {
  if (drafts.unsavedFields.length > 0) return MINIMIZED_DRAFT_LABELS.unsavedEdit;
  if (drafts.composingNote && hasText(drafts.newNoteDraft)) return MINIMIZED_DRAFT_LABELS.unsentNote;
  if (
    (drafts.response && hasText(drafts.response.draft)) ||
    Object.values(drafts.inlineFeedbackReplies).some((replies) => replies.some((reply) => hasText(reply.text))) ||
    Object.values(drafts.feedbackChoiceSelections).some((choices) => choices.length > 0)
  )
    return MINIMIZED_DRAFT_LABELS.reply;
  return undefined;
}

/** Add a minimized reader, replacing an earlier record of the same frame. */
export function addMinimizedReader(list: readonly MinimizedReader[], entry: MinimizedReader): MinimizedReader[] {
  return [...list.filter((item) => item.id !== entry.id), entry];
}

/** The minimized readers a project tab lists, oldest first. */
export function minimizedReadersForTab(list: readonly MinimizedReader[], tabProjectId: string): MinimizedReader[] {
  return list.filter((item) => item.tabProjectId === tabProjectId);
}

/** Take one minimized reader off the list to restore or close it. */
export function takeMinimizedReader(
  list: readonly MinimizedReader[],
  id: string,
): { list: MinimizedReader[]; entry?: MinimizedReader } {
  const entry = list.find((item) => item.id === id);
  return { list: entry ? list.filter((item) => item !== entry) : [...list], entry };
}

/** Take every minimized reader of a project tab, oldest first, to restore or close them all. */
export function takeMinimizedReadersForTab(
  list: readonly MinimizedReader[],
  tabProjectId: string,
): { list: MinimizedReader[]; entries: MinimizedReader[] } {
  return {
    list: list.filter((item) => item.tabProjectId !== tabProjectId),
    entries: minimizedReadersForTab(list, tabProjectId),
  };
}

/** Closing project tabs drops their minimized readers and readers of their tickets. */
export function disposeMinimizedReaders(
  list: readonly MinimizedReader[],
  projectIds: ReadonlySet<string>,
): { retained: MinimizedReader[]; disposed: MinimizedReader[] } {
  const retained: MinimizedReader[] = [],
    disposed: MinimizedReader[] = [];
  for (const item of list)
    (projectIds.has(item.tabProjectId) || projectIds.has(item.projectId) ? disposed : retained).push(item);
  return { retained, disposed };
}

/** Whether closing these minimized readers would lose a draft, so Close All must confirm. */
export function minimizedReadersHaveDrafts(list: readonly MinimizedReader[]): boolean {
  return list.some((item) => minimizedDraftLabel(item.drafts) !== undefined);
}

/**
 * The edit state a reloaded record rebuilds over the current ticket: its composer, replies, and
 * choices for notes that still exist, and its response in progress. Field edits restore from the
 * ticket draft store when the person edits that field again, as after any reload (HS2-RE1PS6).
 */
export function restoredReaderEditState(ticket: FullTicket, drafts: MinimizedReaderDrafts): TicketReaderEditState {
  const edit = ticketReaderEditState(ticket),
    noteIds = new Set([DETAILS_FEEDBACK_ID, ...ticket.notes.map((note) => note.id)]),
    live = <T>(record: Record<string, T>) =>
      Object.fromEntries(Object.entries(record).filter(([id]) => noteIds.has(id)));
  edit.composingNote = drafts.composingNote;
  edit.newNoteDraft = drafts.composingNote ? drafts.newNoteDraft : '';
  edit.inlineFeedbackReplies = live(drafts.inlineFeedbackReplies);
  edit.feedbackChoiceSelections = live(drafts.feedbackChoiceSelections);
  if (drafts.response) {
    edit.editingNoteId = drafts.response.request.id;
    edit.responseRequestId = drafts.response.request.id;
    edit.noteDraft = drafts.response.draft;
  }
  return edit;
}

const INSPECTOR_TABS = new Set<InspectorTab>(['info', 'timeline', 'code-review', 'attachments']);
const stringField = (value: unknown) => (typeof value === 'string' ? value : undefined);

function parseReplies(value: unknown): Record<string, InlineFeedbackReply[]> {
  if (!isRecord(value)) return {};
  return Object.fromEntries(
    Object.entries(value).map(([id, replies]) => [
      id,
      Array.isArray(replies)
        ? replies.flatMap((reply) =>
            isRecord(reply) && typeof reply.offset === 'number' && typeof reply.text === 'string'
              ? [{ offset: reply.offset, text: reply.text }]
              : [],
          )
        : [],
    ]),
  );
}

function parseChoices(value: unknown): Record<string, string[]> {
  if (!isRecord(value)) return {};
  return Object.fromEntries(
    Object.entries(value).map(([id, choices]) => [
      id,
      Array.isArray(choices) ? choices.filter((choice): choice is string => typeof choice === 'string') : [],
    ]),
  );
}

function parseResponse(value: unknown): MinimizedReaderResponse | undefined {
  if (!isRecord(value) || !isRecord(value.request) || typeof value.draft !== 'string') return undefined;
  const request = value.request,
    id = stringField(request.id),
    text = stringField(request.text),
    kind = stringField(request.kind);
  if (!id || text === undefined || kind !== 'feedback_needed') return undefined;
  return {
    request: {
      id,
      kind,
      text,
      created_at: stringField(request.created_at) ?? '',
      edited_at: stringField(request.edited_at) ?? '',
    },
    draft: value.draft,
  };
}

function parseDrafts(value: unknown): MinimizedReaderDrafts {
  const record = isRecord(value) ? value : {},
    response = parseResponse(record.response),
    fields = Array.isArray(record.unsavedFields) ? record.unsavedFields : [];
  return {
    composingNote: record.composingNote === true,
    newNoteDraft: record.composingNote === true ? (stringField(record.newNoteDraft) ?? '') : '',
    inlineFeedbackReplies: parseReplies(record.inlineFeedbackReplies),
    feedbackChoiceSelections: parseChoices(record.feedbackChoiceSelections),
    ...(response ? { response } : {}),
    unsavedFields: fields.filter(
      (field): field is MinimizedReaderField => field === 'details' || field === 'note' || field === 'blocked_reason',
    ),
  };
}

function parseEntry(value: unknown): MinimizedReader | undefined {
  if (!isRecord(value)) return undefined;
  const id = stringField(value.id),
    tabProjectId = stringField(value.tabProjectId),
    projectId = stringField(value.projectId),
    qualifiedId = stringField(value.qualifiedId),
    slug = stringField(value.slug),
    activeTab = stringField(value.activeTab) as InspectorTab | undefined;
  if (!id || !tabProjectId || !projectId || !qualifiedId || !slug) return undefined;
  return {
    id,
    tabProjectId,
    projectId,
    qualifiedId,
    slug,
    title: stringField(value.title) ?? '',
    category: stringField(value.category) ?? 'issue',
    activeTab: activeTab && INSPECTOR_TABS.has(activeTab) ? activeTab : 'info',
    minimizedAt: typeof value.minimizedAt === 'number' ? value.minimizedAt : 0,
    drafts: parseDrafts(value.drafts),
  };
}

/** The stored form of the list: every record without its live frame. */
export function serializeMinimizedReaders(list: readonly MinimizedReader[]): string {
  return JSON.stringify(
    list.map((entry): Omit<MinimizedReader, 'frame'> => ({
      id: entry.id,
      tabProjectId: entry.tabProjectId,
      projectId: entry.projectId,
      qualifiedId: entry.qualifiedId,
      slug: entry.slug,
      title: entry.title,
      category: entry.category,
      activeTab: entry.activeTab,
      minimizedAt: entry.minimizedAt,
      drafts: entry.drafts,
    })),
  );
}

/** Read the stored list, dropping malformed records and duplicate frames. */
export function parseMinimizedReaders(raw: string | null | undefined): MinimizedReader[] {
  if (!raw) return [];
  const parsed = tryParseJson(raw);
  if (!Array.isArray(parsed)) return [];
  const seen = new Set<string>();
  return parsed.flatMap((value) => {
    const entry = parseEntry(value);
    if (!entry || seen.has(entry.id)) return [];
    seen.add(entry.id);
    return [entry];
  });
}

export function loadMinimizedReaders(storage: Pick<Storage, 'getItem'> | undefined): MinimizedReader[] {
  return loadMinimizedRecords(storage, MINIMIZED_READERS_STORAGE_KEY, parseMinimizedReaders);
}

export function saveMinimizedReaders(
  storage: Pick<Storage, 'setItem' | 'removeItem'> | undefined,
  list: readonly MinimizedReader[],
): void {
  saveMinimizedRecords(storage, MINIMIZED_READERS_STORAGE_KEY, list, serializeMinimizedReaders);
}
