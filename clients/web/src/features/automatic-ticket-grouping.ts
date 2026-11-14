import type { Signal } from 'kerfjs';

import type { AiToolDefaults, Api, TicketRow as WireTicketRow, TurnStreamEnvelope } from '../api';
import {
  AutomaticGroupReplyError,
  buildAutomaticGroupPrompt,
  parseAutomaticGroupProposal,
} from '../automatic-ticket-group';
import type { Project } from '../interactions/types';
import type { TicketGroupOption } from '../ticket-groups';

/** Connection ids of private Automatic turns start with this, so they never join a conversation. */
export const AUTOMATIC_GROUP_CONNECTION_PREFIX = 'hotsheet-automatic-group-';
/** How long the AI may take before the turn is interrupted and nothing changes. */
export const AUTOMATIC_GROUP_TIMEOUT_MS = 180_000;

type DriveClient = Pick<Api, 'createToolConnection' | 'sendToolTurn' | 'interruptToolTurn' | 'deleteToolConnection'>;

export interface AutomaticTicketGroupingDependencies {
  project: () => Project | undefined;
  /** The drive client for the open project. */
  driveClient: (project: Project) => DriveClient;
  /** The project's default AI tool selection (`ai.defaults`), or undefined when no tool is available. */
  selection: () => AiToolDefaults | undefined;
  toolLabel: (tool: string) => string;
  groups: () => readonly TicketGroupOption[];
  rowsFor: (slugs: readonly string[]) => WireTicketRow[];
  /** Why the tickets cannot be grouped, such as a provider without group support. */
  disabledReason: (slugs: readonly string[]) => string | undefined;
  /** Move tickets into an existing group: an ordinary, undoable bulk update. */
  assignExisting: (slugs: string[], group: string) => Promise<boolean>;
  /** Create a group by name, then move the tickets into it. */
  assignNew: (name: string, slugs: string[]) => Promise<{ id: string; name: string }>;
  showToast: (message: string) => void;
  /** The running request's busy label for the app's corner status; undefined when idle. */
  busy: Signal<string | undefined>;
  timeoutMs?: number;
  newId?: () => string;
}

interface PendingTurn {
  turnId: string;
  output: string;
  finish: (outcome: { reason: string; message?: string }) => void;
}

const failureMessage = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));
const plural = (count: number) => `${count} ticket${count === 1 ? '' : 's'}`;

/**
 * Add to Group › Automatic (HS2-4C32A0): ask the project's default AI tool, in a private one-shot
 * drive connection that opens nothing, to choose an existing group for the tickets or name a new
 * one. The app's corner status shows the request while it runs. The reply is validated and applied
 * through the ordinary group writes, so Undo reverses it. No tool, a failed or timed-out turn, and
 * an unusable reply each show a toast and change nothing. The connection is closed afterwards.
 */
export function createAutomaticTicketGrouping(dependencies: AutomaticTicketGroupingDependencies) {
  const {
    project,
    driveClient,
    selection,
    toolLabel,
    groups,
    rowsFor,
    disabledReason,
    assignExisting,
    assignNew,
    showToast,
    busy,
    timeoutMs = AUTOMATIC_GROUP_TIMEOUT_MS,
    newId = () => crypto.randomUUID(),
  } = dependencies;
  const pending = new Map<string, PendingTurn>();

  /**
   * Take a streamed turn event that belongs to a private Automatic connection. Returns whether it
   * did, so the caller keeps it out of every conversation.
   */
  function claimTurnEvent(envelope: TurnStreamEnvelope): boolean {
    if (!envelope.connection_id.startsWith(AUTOMATIC_GROUP_CONNECTION_PREFIX)) return false;
    const turn = pending.get(envelope.connection_id);
    if (!turn || (envelope.turn_id !== undefined && envelope.turn_id !== turn.turnId)) return true;
    const event = envelope.event;
    if (event.type === 'output' && typeof event.content === 'string') turn.output += event.content;
    if (event.type === 'done')
      turn.finish({
        reason: typeof event.reason === 'string' ? event.reason : 'failed',
        ...(typeof event.message === 'string' ? { message: event.message } : {}),
      });
    return true;
  }

  /** Send the turn and wait for its `done` event, or the timeout. Resolves the reply text. */
  async function ask(client: DriveClient, current: Project, chosen: AiToolDefaults, prompt: string): Promise<string> {
    const connectionId = `${AUTOMATIC_GROUP_CONNECTION_PREFIX}${newId()}`,
      turnId = newId();
    let timer: ReturnType<typeof setTimeout> | undefined,
      created = false;
    const outcome = new Promise<{ reason: string; message?: string }>((resolve) => {
      pending.set(connectionId, { turnId, output: '', finish: resolve });
      timer = setTimeout(() => {
        resolve({ reason: 'timeout' });
      }, timeoutMs);
    });
    try {
      const connection = await client.createToolConnection({
        tool: chosen.tool,
        checkout: current.id,
        connection_id: connectionId,
        ...(chosen.model ? { model: chosen.model } : {}),
        ...(chosen.effort ? { effort: chosen.effort } : {}),
      });
      created = true;
      if (!connection.actions?.includes('send_turn'))
        throw new Error(`${toolLabel(chosen.tool)} cannot answer a one-shot request.`);
      await client.sendToolTurn(connection.id, prompt, connection.session_id, {}, turnId);
      const result = await outcome;
      if (result.reason === 'timeout') {
        await client.interruptToolTurn(connectionId).catch(() => undefined);
        throw new Error(`${toolLabel(chosen.tool)} did not answer in time.`);
      }
      if (result.reason !== 'completed')
        throw new Error(result.message ?? `${toolLabel(chosen.tool)} stopped before answering.`);
      return pending.get(connectionId)?.output ?? '';
    } finally {
      clearTimeout(timer);
      pending.delete(connectionId);
      if (created) await client.deleteToolConnection(current.id, connectionId).catch(() => undefined);
    }
  }

  /** Run Automatic for the tickets. Resolves whether a group was applied. */
  async function run(slugs: readonly string[]): Promise<boolean> {
    const current = project(),
      targets = [...slugs];
    if (!current || targets.length === 0 || busy.value !== undefined) return false;
    const reason = disabledReason(targets);
    if (reason) {
      showToast(reason);
      return false;
    }
    const chosen = selection();
    if (!chosen) {
      showToast('Automatic grouping needs an AI tool. Set one up in AI settings; no group was changed.');
      return false;
    }
    const label = toolLabel(chosen.tool),
      known = groups(),
      prompt = buildAutomaticGroupPrompt(
        rowsFor(targets).map((row) => ({ slug: row.slug, title: row.title, category: row.category })),
        known,
      );
    busy.value = `Asking ${label} to choose a group…`;
    try {
      let reply: string;
      try {
        reply = await ask(driveClient(current), current, chosen, prompt);
      } catch (failure) {
        showToast(`Automatic grouping failed: ${failureMessage(failure)} No group was changed.`);
        return false;
      }
      let proposal;
      try {
        proposal = parseAutomaticGroupProposal(reply, known);
      } catch (failure) {
        const detail = failure instanceof AutomaticGroupReplyError ? failure.message : failureMessage(failure);
        showToast(`Automatic grouping could not use ${label}’s answer: ${detail} No group was changed.`);
        return false;
      }
      // The project may have changed while the AI was thinking; never write into another one.
      if (project()?.id !== current.id) return false;
      if (proposal.kind === 'existing') {
        if (!(await assignExisting(targets, proposal.id))) return false;
        showToast(`Added ${plural(targets.length)} to “${proposal.name}”.`);
        return true;
      }
      try {
        const created = await assignNew(proposal.name, targets);
        showToast(`Created “${created.name}” and added ${plural(targets.length)}.`);
        return true;
      } catch (failure) {
        showToast(`Automatic grouping could not create “${proposal.name}”: ${failureMessage(failure)}`);
        return false;
      }
    } finally {
      busy.value = undefined;
    }
  }

  return { run, claimTurnEvent };
}
