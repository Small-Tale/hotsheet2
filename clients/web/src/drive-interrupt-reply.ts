import type { ToolConnection } from './api';

/**
 * Merge an interrupt's reply into a project's drive connections (HS2-80HD7K).
 *
 * The reply is the snapshot taken when the server *accepted* the interrupt, so it still reports the turn
 * as busy. The turn's end travels separately on the change stream and can be applied first. A stored
 * connection that has already gone idle, or moved on to another turn, is newer than the reply, so the
 * reply must not put the ended turn back in progress.
 */
export function mergeInterruptReply(connections: readonly ToolConnection[], reply: ToolConnection): ToolConnection[] {
  const stored = connections.find((item) => item.id === reply.id);
  if (stored && (!stored.busy || (stored.turn_id !== undefined && stored.turn_id !== reply.turn_id)))
    return [...connections];
  return connections.filter((item) => item.id !== reply.id).concat(reply);
}
