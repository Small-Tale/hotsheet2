import type { Note } from './api';

/** The NoteCard authorship facets derived from a note's recorded actor (HS2-32QDZ3). */
export interface NoteAuthorship {
  author: string;
  aiAuthored: boolean;
  aiTool?: string;
}

const KNOWN_TOOLS: Record<string, string> = {
  claude: 'Claude',
  codex: 'Codex',
  opencode: 'OpenCode',
  antigravity: 'Antigravity',
  gemini: 'Gemini',
  hotsheet: 'Hot Sheet AI',
};

/**
 * A readable tool name for an AI actor id. Launched sessions use `<tool>-<session>` worker
 * ids, so the leading segment names the tool; an unrecognized id falls back to "AI" rather
 * than echoing an opaque session token.
 */
export function aiToolName(id: string | undefined): string {
  const prefix =
    id
      ?.trim()
      .toLowerCase()
      .split(/[-_:\s]/u)[0] ?? '';
  return KNOWN_TOOLS[prefix] ?? 'AI';
}

/**
 * Who wrote a note: an `ai` actor is labeled AI-generated with its tool, a `human` actor
 * shows its id (or "Human"), and system or unrecorded authorship stays "Hot Sheet".
 * Activity-distillation notes are Hot Sheet's own AI output.
 */
export function noteAuthorship(note: Pick<Note, 'actor' | 'text'>): NoteAuthorship {
  if (note.text.includes('hotsheet:activity-distillation:v1:'))
    return { author: 'Hot Sheet AI', aiAuthored: true, aiTool: 'Hot Sheet AI' };
  const actor = note.actor;
  if (actor?.role === 'ai') {
    const tool = aiToolName(actor.id);
    return { author: tool, aiAuthored: true, aiTool: tool };
  }
  if (actor?.role === 'human')
    return {
      author: actor.id?.startsWith('feedback-rater:') ? 'Human' : actor.id?.trim() || 'Human',
      aiAuthored: false,
    };
  return { author: 'Hot Sheet', aiAuthored: false };
}
