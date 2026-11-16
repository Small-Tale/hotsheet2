import { type Signal, signal } from 'kerfjs';

import { Api, type CodeReview, type FullTicket } from '../api';
import type { TicketFollowUpLinkDialogState } from '../components/ticket-follow-up-link-dialog';
import type { Project } from '../interactions/types';
import { duplicateTargetKey } from '../ticket-close';
import {
  followUpEvidenceKeys,
  followUpLinkedMessage,
  type FollowUpLinkTicket,
  followUpQualifiedReference,
  isLinkedFollowUp,
  validateFollowUpLink,
} from '../ticket-follow-up-link';
import { searchTicketReferences, ticketReference } from '../ticket-reference-search';
import { type LinkedReaderSubject, WORKSPACE_READER_SCOPE } from './linked-reader-subject';

/** Live application bindings the follow-up link flow reads (HS2-MCGCPQ). */
export interface FollowUpLinkDependencies {
  project: () => Project | undefined;
  projects: Signal<Project[]>;
  selectedTicket: Signal<FullTicket | null>;
  codeReview: Signal<CodeReview | undefined>;
  refreshCodeReview: () => Promise<void>;
  showToast: (message: string) => void;
  /**
   * A linked reader frame's own ticket, project, and Code Review (HS2-NXSMC7). The Inspector and the
   * workspace reader act on the selected ticket, so this returns undefined for them.
   */
  linkedReader?: (frameId: string) => LinkedReaderSubject | undefined;
  /** Search debounce after the coalesced input settles; tests pass 0. */
  searchDelayMs?: number;
}

/** The ticket a link dialog records follow-ups for, and how to refresh its Code Review. */
interface FollowUpLinkSubject {
  project: Project;
  ticket: FullTicket;
  codeReview?: CodeReview;
  refreshCodeReview: () => Promise<void>;
}

export type FollowUpLinkController = ReturnType<typeof createFollowUpLinkController>;

/**
 * The Link Existing Ticket as Follow-Up flow of the Inspector (HS2-MCGCPQ) and the modal reader
 * (HS2-NXSMC7): open the dialog for the inspected or read ticket, search open projects for the
 * target, and record the relation through the idempotent `follow-up-links` route of the target's
 * checkout. Success refreshes that surface's Code Review so the new row appears; a refusal stays
 * in the dialog where the person can correct it. A dialog opened from a reader frame carries its
 * `readerScope` and renders inside that reader.
 */
export function createFollowUpLinkController(dependencies: FollowUpLinkDependencies) {
  const { project, projects, selectedTicket, codeReview, refreshCodeReview, showToast, linkedReader } = dependencies,
    searchDelayMs = dependencies.searchDelayMs ?? 150,
    dialog = signal<TicketFollowUpLinkDialogState | undefined>(undefined);
  let generation = 0,
    timer: ReturnType<typeof setTimeout> | undefined;

  /** The dialog still open for the same source after an await, which may have closed or replaced it. */
  function openFor(state: TicketFollowUpLinkDialogState): TicketFollowUpLinkDialogState | undefined {
    const active = dialog.peek();
    return active && duplicateTargetKey(active.source) === duplicateTargetKey(state.source) ? active : undefined;
  }
  function cancelSearch() {
    generation += 1;
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  }
  function close() {
    cancelSearch();
    dialog.value = undefined;
  }
  /** The ticket of the Inspector or workspace reader, or of the linked reader frame `readerScope`. */
  function subject(readerScope?: string): FollowUpLinkSubject | undefined {
    if (readerScope && readerScope !== WORKSPACE_READER_SCOPE) return linkedReader?.(readerScope);
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket) return undefined;
    return { project: current, ticket, codeReview: codeReview.value, refreshCodeReview };
  }
  /**
   * Open the dialog for the ticket of the surface that `readerScope` names (a reader frame id, or
   * undefined for the Inspector); returns false when that surface shows no ticket.
   */
  function open(readerScope?: string): boolean {
    const owner = subject(readerScope);
    if (!owner) return false;
    cancelSearch();
    dialog.value = {
      source: ticketReference(owner.project, owner.ticket),
      existing: followUpEvidenceKeys(owner.codeReview?.follow_ups),
      query: '',
      candidates: [],
      ...(readerScope ? { readerScope } : {}),
    };
    return true;
  }
  /** The dialog state that surface renders: a reader frame id, or undefined for the app root. */
  function dialogFor(readerScope?: string): TicketFollowUpLinkDialogState | undefined {
    const state = dialog.value;
    return state?.readerScope === readerScope ? state : undefined;
  }
  function search(query: string) {
    const state = dialog.value;
    if (!state) return;
    cancelSearch();
    const active = generation;
    dialog.value = {
      ...state,
      query,
      selected: undefined,
      candidates: [],
      searching: Boolean(query.trim()),
      error: '',
    };
    if (!query.trim()) return;
    timer = setTimeout(() => {
      timer = undefined;
      void searchTicketReferences(projects.value, query, (item) =>
        new Api(item.apiPath).checkoutTickets(item.id, { text: query, compact: true, limit: 500 }),
      ).then(({ candidates, errors }) => {
        const latest = dialog.value;
        if (active !== generation || !latest || duplicateTargetKey(latest.source) !== duplicateTargetKey(state.source))
          return;
        dialog.value = { ...latest, candidates, searching: false, error: errors.join(' · ') };
      });
    }, searchDelayMs);
  }
  function select(key: string) {
    const state = dialog.value,
      candidate = state?.candidates.find((item) => duplicateTargetKey(item) === key);
    if (!state || !candidate) return;
    cancelSearch();
    dialog.value = { ...state, selected: candidate, query: '', candidates: [], searching: false, error: '' };
  }
  function clear() {
    const state = dialog.value;
    if (!state) return;
    cancelSearch();
    dialog.value = { ...state, selected: undefined, query: '', candidates: [], searching: false, error: '' };
  }
  async function submit() {
    const state = dialog.value;
    if (!state || state.submitting) return;
    const validation = validateFollowUpLink(state.source, state.selected);
    if (validation) {
      dialog.value = { ...state, error: validation };
      return;
    }
    const target = state.selected as FollowUpLinkTicket,
      owner = projects.value.find((item) => item.id === target.projectId);
    if (!owner) {
      dialog.value = { ...state, error: `Open ${target.projectName} to link ${target.slug}.` };
      return;
    }
    cancelSearch();
    dialog.value = { ...state, submitting: true, error: '' };
    try {
      await new Api(owner.apiPath).linkCheckoutFollowUp(
        owner.id,
        followUpQualifiedReference(state.source),
        followUpQualifiedReference(target),
      );
      // The link is recorded even when the dialog was dismissed meanwhile, so always confirm it.
      if (openFor(state)) close();
      showToast(followUpLinkedMessage(state.source, target, isLinkedFollowUp(state.existing, target)));
      const surface = subject(state.readerScope);
      if (surface?.project.id === state.source.projectId && surface.ticket.qualified_id === state.source.qualifiedId)
        await surface.refreshCodeReview();
    } catch (reason) {
      const active = openFor(state);
      if (active)
        dialog.value = {
          ...active,
          submitting: false,
          error: reason instanceof Error ? reason.message : String(reason),
        };
    }
  }
  return { dialog, dialogFor, open, search, select, clear, submit, close, cancelSearch };
}
