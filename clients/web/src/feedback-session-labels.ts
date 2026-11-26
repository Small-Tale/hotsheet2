/**
 * How the "Choose an AI Session" prompt names each candidate session (HS2-5MCAKY): by the terminal or
 * AI chat title the user sees in the drawer, its project, and the ticket its worker has claimed,
 * instead of an opaque session id. The id stays available as the secondary description.
 */

/** A candidate as the server offers it: the live connection id, its tool, and its claim worker. */
export interface FeedbackSessionCandidate {
  id: string;
  tool: string;
  worker?: string;
}

export interface FeedbackSessionContext {
  projectName: string;
  /** The project's terminal tabs, by terminal id, with their shown titles. */
  terminals: ReadonlyArray<{ id: string; title?: string }>;
  /** The project's AI chat tabs: a drive candidate's id is the chat's connection id. */
  chats: ReadonlyArray<{ id: string; connectionId: string; name: string }>;
  /** Tickets with a live claim: which worker holds which ticket. */
  claims: ReadonlyArray<{ worker: string; slug: string; title: string }>;
  /** The tool's display name (`claude` → `Claude`). */
  toolLabel: (tool: string) => string;
}

export interface FeedbackSessionLabel {
  /** The button text: `Claude 1 · demo · HS2-ABC123`. */
  label: string;
  /** Secondary detail for a tooltip and screen readers, including the session id. */
  description: string;
}

export function feedbackSessionLabel(
  session: FeedbackSessionCandidate,
  context: FeedbackSessionContext,
): FeedbackSessionLabel {
  const terminal = context.terminals.find((item) => item.id === session.id),
    chat = context.chats.find((item) => item.connectionId === session.id || item.id === session.id),
    title = terminal?.title?.trim() || chat?.name.trim() || context.toolLabel(session.tool),
    claim = session.worker ? context.claims.find((item) => item.worker === session.worker) : undefined,
    label = [title, context.projectName, claim?.slug].filter(Boolean).join(' · '),
    working = claim ? `working on ${claim.slug} ${claim.title}` : 'no claimed ticket';
  return {
    label,
    description: `${context.toolLabel(session.tool)} in ${context.projectName}, ${working} (session ${session.id})`,
  };
}
