import type { ActivityEvent } from './api';

/**
 * What the announcer says aloud for an activity event (HS2-R5Q2GY): a short summary of what was done,
 * never a literal command. Commands become a deterministic phrase by what the program does; other
 * summaries keep their prose and replace inline code, code blocks, and links with plain words.
 */
export function announcerSpokenText(event: Pick<ActivityEvent, 'tool' | 'kind' | 'summary' | 'detail'>): string {
  const tool = event.tool.trim() || 'The agent';
  if (event.kind === 'command') return `${tool} ${commandPhrase(commandText(event))}.`;
  if (event.kind === 'edit') {
    const path = detailString(event.detail, 'path');
    if (path && /^\S+ edited /.test(event.summary.trim())) return `${tool} edited ${basename(path)}.`;
  }
  const spoken = speakableProse(event.summary);
  return spoken || `${tool} reported ${event.kind.replaceAll('_', ' ')}.`;
}

function detailString(detail: unknown, key: string): string | undefined {
  if (detail === null || typeof detail !== 'object' || Array.isArray(detail)) return undefined;
  const value = (detail as Record<string, unknown>)[key];
  return typeof value === 'string' && value.trim() ? value.trim() : undefined;
}

/** The command an event ran: its detail, else the first inline code span of its summary. */
function commandText(event: Pick<ActivityEvent, 'summary' | 'detail'>): string {
  return detailString(event.detail, 'command') ?? /`([^`]+)`/.exec(event.summary)?.[1] ?? '';
}

const basename = (path: string) =>
  path
    .replace(/[/\\]+$/, '')
    .split(/[/\\]/)
    .pop() ?? path;

/** The words of a command after shell wrappers (`/bin/zsh -lc "…"`), env assignments, and `sudo`. */
function commandWords(command: string): string[] {
  let text = command.trim();
  const wrapped = /^(?:\S*\/)?(?:ba|z)?sh\s+-l?c\s+(["'])([\s\S]*)\1\s*$/.exec(text);
  if (wrapped?.[2]) text = wrapped[2];
  const words = text.split(/\s+/).filter(Boolean);
  while (words.length && (/^\w+=/.test(words[0] ?? '') || words[0] === 'sudo' || words[0] === 'env')) words.shift();
  return words.map((word) => word.replace(/^["']|["']$/g, ''));
}

/** A spoken phrase for what a command does, chosen from its program and arguments. */
export function commandPhrase(command: string): string {
  const words = commandWords(command);
  const program = basename(words[0] ?? '').toLowerCase();
  const args = words.slice(1).map((word) => word.toLowerCase());
  const has = (...names: string[]) => names.some((name) => args.includes(name));
  if (!program) return 'ran a command';
  if (program === 'git') {
    if (has('commit')) return 'committed changes';
    if (has('push')) return 'pushed changes';
    if (has('pull', 'fetch')) return 'updated from the remote';
    if (has('rebase', 'merge', 'cherry-pick')) return 'integrated changes';
    if (has('status', 'diff', 'log', 'show')) return 'checked the repository';
    return 'used Git';
  }
  if (/hotsheet/.test(program)) return 'updated Hot Sheet tickets';
  if (/^(?:curl|wget|gh)$/.test(program)) return 'made a network request';
  // Whole words only, so a host name or path that merely contains "test" does not count.
  const tokens = [program, ...args.map((arg) => basename(arg))];
  const uses = (pattern: RegExp) => tokens.some((token) => pattern.test(token));
  if (uses(/^(?:test|tests|nextest|vitest|jest|pytest|playwright|check\.sh|test:[\w:-]+)$/)) return 'ran tests';
  if (uses(/^(?:lint|lint:[\w:-]+|eslint|clippy|prettier|fmt|format)$/)) return 'checked formatting and lint';
  if (uses(/^(?:build|tsc|compile)$/)) return 'built the project';
  if (/^(?:npm|pnpm|yarn|pip|pip3|cargo|brew)$/.test(program) && /^(?:install|i|ci|add)$/.test(args[0] ?? ''))
    return 'installed dependencies';
  if (/^(?:rg|grep|find|fd|ag)$/.test(program)) return 'searched the code';
  if (/^cat$/.test(program) && args.some((arg) => arg.startsWith('>'))) return 'wrote a file';
  if (/^(?:cat|sed|head|tail|less|nl|wc|ls|tree|bat)$/.test(program)) return 'read files';
  if (/^(?:mv|cp|rm|mkdir|touch|ln)$/.test(program)) return 'rearranged files';
  return 'ran a command';
}

/** Summary prose fit to say aloud: inline code becomes its plain name or "a command". */
export function speakableProse(summary: string): string {
  return summary
    .replace(/```[\s\S]*?(?:```|$)/g, ' ')
    .replace(/`([^`]*)`/g, (_match, code: string) => speakableCode(code))
    .replace(/`/g, '')
    .replace(/\bhttps?:\/\/\S+/g, 'a link')
    .replace(/(\*\*|__|~~)/g, '')
    .replace(/(^|\s)#{1,6}\s+/g, '$1')
    .replace(/\s+/g, ' ')
    .trim();
}

function speakableCode(code: string): string {
  const trimmed = code.trim();
  if (!trimmed) return '';
  if (/^[\w.@-]{1,32}$/.test(trimmed)) return trimmed;
  if (/^[\w.@/\\-]{1,200}$/.test(trimmed) && /[/\\]/.test(trimmed)) return basename(trimmed);
  return 'a command';
}
