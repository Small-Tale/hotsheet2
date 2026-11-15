import { effect, type Signal, signal } from 'kerfjs';

import { Api, type Capabilities, type FullTicket, type TicketRow } from '../api';
import type { TicketBlockerDialogState } from '../components/ticket-blocker-dialog';
import type { TicketBlockersProps } from '../components/ticket-blockers';
import type { Project } from '../interactions/types';
import {
  blockerAddedMessage,
  blockerAvailability,
  blockerCandidates,
  blockerReference,
  blockerRemovedMessage,
  blockerRows,
  type BlockerSummary,
  type BlockerTicket,
  validateBlocker,
  withBlocker,
  withoutBlocker,
} from '../ticket-blockers';
import { duplicateTargetKey } from '../ticket-close';
import { isTicketConcurrencyConflict } from '../ticket-field-reconciliation';
import { searchTicketReferences, ticketReference } from '../ticket-reference-search';

/** Live application bindings the Blocked By section and its dialog read (HS2-CHH28E). */
export interface TicketBlockersDependencies {
  project: () => Project | undefined;
  selectedTicket: Signal<FullTicket | null>;
  /** The selected project's loaded rows; a blocker found here needs no request. */
  tickets: Signal<TicketRow[]>;
  capabilitiesFor: (connectionId: string) => Capabilities | undefined;
  /** The display name of a project's ticket source, for disabled reasons. */
  sourceName: (projectId: string, connectionId: string) => string | undefined;
  canUpdateSelected: () => boolean;
  showToast: (message: string) => void;
  /** Search debounce after the coalesced input settles; tests pass 0. */
  searchDelayMs?: number;
}

export type TicketBlockersController = ReturnType<typeof createTicketBlockersController>;

/**
 * The Inspector's Blocked By editor (HS2-CHH28E). It projects the selected ticket's
 * `blocked_by` into rows, resolving blockers that are not loaded through the checkout route,
 * and adds or removes one blocker at a time. Each write rereads the ticket and replaces the set
 * with its concurrency token, retrying once after a concurrent edit, so a blocker another
 * client added meanwhile is kept. A refusal (a dependency cycle, a self-block) stays in the
 * dialog; a failed removal is reported in a toast.
 */
export function createTicketBlockersController(dependencies: TicketBlockersDependencies) {
  const { project, selectedTicket, tickets, capabilitiesFor, sourceName, canUpdateSelected, showToast } = dependencies,
    searchDelayMs = dependencies.searchDelayMs ?? 150,
    dialog = signal<TicketBlockerDialogState | undefined>(undefined),
    /** Blockers resolved through the route, keyed by reference; `null` did not resolve. */
    resolved = signal<Record<string, BlockerSummary | null>>({}),
    removing = signal<string[]>([]);
  let generation = 0,
    timer: ReturnType<typeof setTimeout> | undefined;
  const pending = new Set<string>();

  function loadedRow(ticket: FullTicket, nativeId: string): TicketRow | undefined {
    return tickets.value.find((row) => row.connection_id === ticket.connection_id && row.native_id === nativeId);
  }
  /** Resolve the selected ticket's blockers that are not among the loaded rows. */
  function resolveMissing(current: Project, ticket: FullTicket) {
    for (const nativeId of ticket.blocked_by) {
      const reference = blockerReference(current.id, ticket.connection_id, nativeId);
      if (loadedRow(ticket, nativeId) || reference in resolved.peek() || pending.has(reference)) continue;
      pending.add(reference);
      void new Api(current.apiPath)
        .checkoutTicket(current.id, `${ticket.connection_id}:${nativeId}`)
        .then(
          ({ ticket: found }) => found as BlockerSummary | null,
          () => null,
        )
        .then((found) => {
          pending.delete(reference);
          resolved.value = { ...resolved.peek(), [reference]: found };
        });
    }
  }
  const stopResolving = effect(() => {
    const ticket = selectedTicket.value,
      current = project();
    if (ticket && current && ticket.blocked_by.length > 0) resolveMissing(current, ticket);
  });

  /** The Blocked By section's props for the inspected ticket. */
  function sectionProps(): TicketBlockersProps | undefined {
    const ticket = selectedTicket.value,
      current = project();
    if (!ticket || !current) return undefined;
    const capabilities = capabilitiesFor(ticket.connection_id),
      name = sourceName(current.id, ticket.connection_id) ?? ticket.connection_id,
      cache = resolved.value;
    return {
      slug: ticket.slug,
      rows: blockerRows(
        ticket,
        current.id,
        (nativeId) =>
          loadedRow(ticket, nativeId) ??
          cache[blockerReference(current.id, ticket.connection_id, nativeId)] ??
          undefined,
      ),
      availability: blockerAvailability(capabilities ? { name, capabilities } : undefined, canUpdateSelected()),
      removing: removing.value,
    };
  }

  /** Reread the ticket and replace its blockers with `change` applied to the current set. */
  async function writeBlockers(current: Project, ticket: FullTicket, change: (blockedBy: string[]) => string[]) {
    const client = new Api(current.apiPath);
    for (let attempt = 0; ; attempt += 1) {
      const fresh = (await client.checkoutTicket(current.id, ticket.qualified_id)).ticket;
      try {
        const updated = (
          await client.updateCheckoutTicket(current.id, ticket.qualified_id, {
            blocked_by: change(fresh.blocked_by),
            ...(fresh.concurrency_token ? { expected_token: fresh.concurrency_token } : {}),
          })
        ).ticket;
        adopt(current, updated);
        return updated;
      } catch (reason) {
        if (attempt > 0 || !isTicketConcurrencyConflict(reason)) throw reason;
      }
    }
  }
  /** Show the server's answer in the Inspector and the list without waiting for the change stream. */
  function adopt(current: Project, updated: FullTicket) {
    if (project()?.id !== current.id) return;
    tickets.value = tickets.value.map((row) =>
      row.qualified_id === updated.qualified_id ? { ...row, blocked_by: updated.blocked_by } : row,
    );
    const selected = selectedTicket.value;
    if (selected?.qualified_id === updated.qualified_id) selectedTicket.value = updated;
  }

  async function remove(nativeId: string) {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket || removing.value.includes(nativeId)) return;
    const name =
      sectionProps()?.rows.find((row) => row.nativeId === nativeId)?.slug ??
      loadedRow(ticket, nativeId)?.slug ??
      nativeId;
    removing.value = [...removing.value, nativeId];
    try {
      await writeBlockers(current, ticket, (blockedBy) => withoutBlocker(blockedBy, nativeId));
      showToast(blockerRemovedMessage(ticket.slug, name));
    } catch (reason) {
      showToast(`Could not remove ${name}: ${reason instanceof Error ? reason.message : String(reason)}`);
    } finally {
      removing.value = removing.value.filter((item) => item !== nativeId);
    }
  }

  /** The dialog still open for the same source after an await, which may have closed or replaced it. */
  function openFor(state: TicketBlockerDialogState): TicketBlockerDialogState | undefined {
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
  /** Open the dialog for the inspected ticket; returns false when it cannot record blockers. */
  function open(): boolean {
    const current = project(),
      ticket = selectedTicket.value;
    if (!current || !ticket || !sectionProps()?.availability.enabled) return false;
    cancelSearch();
    dialog.value = {
      source: ticketReference(current, ticket),
      existing: [...ticket.blocked_by],
      query: '',
      candidates: [],
    };
    return true;
  }
  function search(query: string) {
    const state = dialog.value,
      current = project();
    if (!state || !current) return;
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
      // Blockers are source-local, so only the inspected project is searched.
      void searchTicketReferences([current], query, (item) =>
        new Api(item.apiPath).checkoutTickets(item.id, { text: query, compact: true, limit: 500 }),
      ).then(({ candidates, errors }) => {
        const latest = dialog.value;
        if (active !== generation || !latest || !openFor(state)) return;
        dialog.value = {
          ...latest,
          candidates: blockerCandidates(latest.source, candidates),
          searching: false,
          error: errors.join(' · '),
        };
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
    const validation = validateBlocker(state.source, state.selected, state.existing);
    if (validation) {
      dialog.value = { ...state, error: validation };
      return;
    }
    const target = state.selected as BlockerTicket,
      current = project(),
      ticket = selectedTicket.value;
    if (current?.id !== state.source.projectId || ticket?.qualified_id !== state.source.qualifiedId) {
      dialog.value = { ...state, error: `Select ${state.source.slug} again to add its blocker.` };
      return;
    }
    cancelSearch();
    dialog.value = { ...state, submitting: true, error: '' };
    try {
      await writeBlockers(current, ticket, (blockedBy) => withBlocker(blockedBy, target.nativeId));
      // The blocker is recorded even when the dialog was dismissed meanwhile, so always confirm it.
      if (openFor(state)) close();
      showToast(blockerAddedMessage(state.source, target));
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
  function dispose() {
    cancelSearch();
    stopResolving();
  }
  return { dialog, sectionProps, open, search, select, clear, submit, close, remove, cancelSearch, dispose };
}
