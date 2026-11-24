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

/** A command split at unquoted `&&`, `||`, `;` and `|`; `piped` marks a segment that reads a pipe. */
interface CommandSegment {
  words: string[];
  piped: boolean;
}

/** The command inside a shell wrapper (`/bin/zsh -lc "…"`), else the command itself. */
function unwrapShell(command: string): string {
  const text = command.trim();
  const wrapped = /^(?:\S*\/)?(?:ba|z)?sh\s+-l?c\s+(["'])([\s\S]*)\1\s*$/.exec(text);
  return wrapped?.[2] ?? text;
}

/** Split a command line into simple commands, keeping quoted text whole (HS2-RKYCTP). */
function commandSegments(command: string): CommandSegment[] {
  const segments: CommandSegment[] = [];
  let words: string[] = [],
    word = '',
    quote: string | undefined,
    piped = false;
  const endWord = () => {
    if (word) words.push(word);
    word = '';
  };
  const endSegment = (nextPiped: boolean) => {
    endWord();
    if (words.length) segments.push({ words, piped });
    words = [];
    piped = nextPiped;
  };
  const text = unwrapShell(command);
  for (let index = 0; index < text.length; index++) {
    const char = text[index];
    if (quote) {
      if (char === quote) quote = undefined;
      else word += char;
      continue;
    }
    if (char === '"' || char === "'") {
      quote = char;
      continue;
    }
    if (char === '&' && text[index + 1] === '&') {
      endSegment(false);
      index++;
    } else if (char === '|') {
      const double = text[index + 1] === '|';
      endSegment(!double);
      if (double) index++;
    } else if (char === ';' || char === '\n') endSegment(false);
    else if (/\s/.test(char)) endWord();
    else word += char;
  }
  // An unterminated quote means the text is not a command line this can read.
  if (quote) return [];
  endSegment(false);
  return segments;
}

/** A segment's words after env assignments, `sudo`, `env`, `time`, and `npx`/`exec` wrappers. */
function segmentWords(words: readonly string[]): string[] {
  const rest = [...words];
  while (rest.length && (/^\w+=/.test(rest[0]) || ['sudo', 'env', 'time', 'exec', 'npx', 'command'].includes(rest[0])))
    rest.shift();
  return rest;
}

/** A plain word a voice can say: letters only, so no path, flag, or code is spoken. */
const speakableWord = (word: string | undefined, max = 24): word is string =>
  word !== undefined && new RegExp(`^[A-Za-z][A-Za-z ]{1,${max - 1}}$`).test(word);
/** A file's base name when the argument names a file (`src/a.ts` speaks as `a.ts`). */
function fileName(arg: string): string | undefined {
  if (arg.startsWith('-') || arg.startsWith('$') || /[<>*?{}[\]=]/.test(arg)) return undefined;
  const name = basename(arg);
  return /^[A-Za-z_][\w-]*\.[A-Za-z0-9]{1,8}$/.test(name) ? name : undefined;
}
const positional = (args: readonly string[]) => args.filter((arg) => !arg.startsWith('-'));

/** Programs that only filter a pipe; after a `|` they add nothing worth saying. */
const PIPE_FILTERS = new Set([
  'head',
  'tail',
  'sort',
  'uniq',
  'wc',
  'grep',
  'rg',
  'cut',
  'tr',
  'jq',
  'less',
  'cat',
  'sed',
  'awk',
  'xargs',
  'tee',
  'column',
  'nl',
]);
/** Shell housekeeping that says nothing about the work. */
const NOISE = new Set([
  'cd',
  'pushd',
  'popd',
  'export',
  'echo',
  'printf',
  'set',
  'true',
  'false',
  'sleep',
  'source',
  '.',
  'clear',
  'unset',
  'wait',
]);

function gitPhrase(args: readonly string[]): string {
  // `git -C <dir> …` names its directory before the subcommand.
  const rest = [...args];
  while (rest.length && rest[0].startsWith('-')) rest.splice(0, rest[0] === '-C' || rest[0] === '-c' ? 2 : 1);
  const [sub, ...more] = rest,
    targets = positional(more);
  switch (sub) {
    case 'commit':
      return 'committed changes';
    case 'push': {
      const branch = targets.at(-1)?.split(':').pop();
      return targets.length > 1 && speakableWord(branch) ? `pushed changes to ${branch}` : 'pushed changes';
    }
    case 'pull':
    case 'fetch':
      return 'updated from the remote';
    case 'rebase':
    case 'merge':
    case 'cherry-pick':
      return 'integrated changes';
    case 'status':
      return 'checked what changed';
    case 'diff':
      return more.includes('--cached') || more.includes('--staged')
        ? 'reviewed the staged changes'
        : 'reviewed the changes';
    case 'log':
      return 'read the commit history';
    case 'show':
      return 'looked at a commit';
    case 'add':
      return 'staged changes';
    case 'checkout':
    case 'switch':
      return 'switched branches';
    case 'branch':
      return 'checked the branches';
    case 'worktree':
      return 'managed worktrees';
    case 'stash':
      return 'stashed changes';
    case 'reset':
    case 'restore':
      return 'reset changes';
    case 'clone':
      return 'cloned a repository';
    case 'blame':
      return 'checked who changed the code';
    default:
      return 'used Git';
  }
}

function hotSheetPhrase(args: readonly string[]): string {
  const rest = [...args];
  // `-C <store>` comes before the subcommand.
  while (rest.length && rest[0].startsWith('-')) rest.splice(0, rest[0] === '-C' || rest[0] === '--path' ? 2 : 1);
  const [sub, ...more] = rest;
  const status = more[more.indexOf('--status') + 1];
  switch (sub) {
    case 'claim':
    case 'claim-next':
      return 'claimed a ticket';
    case 'release':
      return 'released a ticket';
    case 'renew':
      return 'renewed a ticket claim';
    case 'new':
      return 'created a ticket';
    case 'show':
      return 'read a ticket';
    case 'ls':
      return 'listed tickets';
    case 'attach':
      return 'attached files to a ticket';
    case 'edit':
      if (more.includes('--status') && status === 'completed') return 'completed a ticket';
      if (more.includes('--note') || more.includes('--note-file')) return 'added a ticket note';
      return 'updated a ticket';
    default:
      return 'updated Hot Sheet tickets';
  }
}

const TEST_RUNNERS: ReadonlyArray<[RegExp, string]> = [
  [/^(?:nextest)$/, 'ran the Rust tests'],
  [/^playwright$/, 'ran the browser tests'],
  [/^(?:vitest|jest|mocha)$/, 'ran the unit tests'],
  [/^pytest$/, 'ran the Python tests'],
  [/^check\.sh$/, 'ran the project checks'],
];

/** The phrase for one simple command, or undefined when it says nothing about the work. */
function segmentPhrase(segment: CommandSegment): string | undefined {
  const words = segmentWords(segment.words);
  const program = basename(words[0] ?? '').toLowerCase();
  const args = words.slice(1);
  const lower = args.map((word) => word.toLowerCase());
  const has = (...names: string[]) => names.some((name) => lower.includes(name));
  if (!program || NOISE.has(program)) return undefined;
  if (segment.piped && PIPE_FILTERS.has(program) && !args.some((arg) => fileName(arg))) return undefined;
  if (program === 'git') return gitPhrase(args);
  if (/hotsheet/.test(program)) return hotSheetPhrase(args);
  if (program === 'gh') {
    if (has('pr')) return 'worked on a pull request';
    if (has('run', 'workflow')) return 'checked CI';
    if (has('issue')) return 'worked on an issue';
    return 'used GitHub';
  }
  if (/^(?:curl|wget|http)$/.test(program)) return 'made a network request';
  const tokens = [program, ...lower.map((arg) => basename(arg))];
  for (const [pattern, phrase] of TEST_RUNNERS) if (tokens.some((token) => pattern.test(token))) return phrase;
  if (program === 'cargo') {
    if (has('test')) return 'ran the Rust tests';
    if (has('clippy')) return 'linted the Rust code';
    if (has('fmt')) return 'checked the formatting';
    if (has('build', 'check')) return 'built the Rust code';
    if (has('run')) return 'ran the Rust program';
  }
  // npm/pnpm/yarn scripts: `test`, `lint`, `build`, or the script's own plain name.
  if (/^(?:npm|pnpm|yarn|bun)$/.test(program)) {
    const verb = lower.at(0) ?? '',
      script = verb === 'run' ? (lower.at(1) ?? '') : verb;
    if (/^(?:install|i|ci|add)$/.test(verb)) return 'installed dependencies';
    if (/^test(?::|$)/.test(script)) return 'ran the tests';
    if (/^lint(?::|$)/.test(script)) return 'linted the code';
    if (/^(?:format|fmt)(?::|$)/.test(script)) return 'checked the formatting';
    if (/^(?:build|typecheck)(?::|$)/.test(script)) return 'built the project';
    return speakableWord(script) ? `ran the ${script} script` : 'ran a project script';
  }
  if (/^(?:pip|pip3|brew)$/.test(program) && /^(?:install|add)$/.test(lower[0] ?? '')) return 'installed dependencies';
  if (program === 'eslint') return 'linted the code';
  if (/^(?:prettier|rustfmt)$/.test(program)) return 'checked the formatting';
  if (program === 'tsc') return 'type-checked the code';
  if (/^(?:rg|grep|ag|ack)$/.test(program)) {
    if (has('--files', '-l', '--files-with-matches')) return 'looked for files';
    const pattern = positional(args)[0];
    return speakableWord(pattern) ? `searched the code for ${pattern}` : 'searched the code';
  }
  if (/^(?:find|fd)$/.test(program)) return 'looked for files';
  const files = args.map(fileName).filter((name): name is string => Boolean(name));
  const redirect = args.findIndex((arg) => arg === '>' || arg === '>>' || /^>>?\S/.test(arg));
  if (redirect >= 0 || program === 'tee') {
    const target = program === 'tee' ? positional(args)[0] : args[redirect].replace(/^>>?/, '') || args[redirect + 1];
    const name = target ? fileName(target) : undefined;
    return name ? `wrote ${name}` : 'wrote a file';
  }
  if (/^(?:cat|sed|head|tail|less|nl|bat|more|awk)$/.test(program)) {
    if (files.length === 1) return `read ${files[0]}`;
    return files.length > 1 ? `read ${files.length} files` : 'read files';
  }
  if (/^(?:ls|tree|exa|eza)$/.test(program)) return 'listed files';
  if (program === 'wc') return 'counted lines';
  if (program === 'mkdir') return 'created a folder';
  if (program === 'touch') return files.length === 1 ? `created ${files[0]}` : 'created a file';
  if (program === 'rm') return files.length === 1 ? `deleted ${files[0]}` : 'deleted files';
  if (program === 'mv') return files.length >= 1 ? `moved ${files[0]}` : 'moved files';
  if (program === 'cp') return files.length >= 1 ? `copied ${files[0]}` : 'copied files';
  if (/^(?:ln|chmod|chown)$/.test(program)) return 'changed file settings';
  if (/^(?:python|python3)$/.test(program)) return 'ran a Python script';
  if (/^(?:node|tsx|deno|bun)$/.test(program)) return 'ran a Node script';
  if (/^(?:kill|pkill|killall)$/.test(program)) return 'stopped a process';
  if (/^(?:ps|lsof|pgrep|top)$/.test(program)) return 'checked running processes';
  if (/^(?:df|du)$/.test(program)) return 'checked disk space';
  if (program === 'jq') return 'read JSON';
  if (program === 'open') return 'opened a file';
  // A script by its plain name (`./scripts/sync-docs.sh` → "ran the sync docs script").
  const stem = program.replace(/\.(?:sh|bash|zsh|mjs|js|py|rb)$/, '');
  if (stem !== program) {
    const spoken = stem.replace(/[-_]+/g, ' ');
    return speakableWord(spoken, 32) ? `ran the ${spoken} script` : 'ran a script';
  }
  return speakableWord(program, 20) ? `ran ${program}` : undefined;
}

/**
 * A spoken phrase for what a command does (HS2-R5Q2GY, HS2-RKYCTP): each simple command of a compound
 * line is named by what it does, housekeeping (`cd`, `echo`, pipe filters) is dropped, and at most two
 * distinct phrases are said. Paths, flags, quoted arguments, and the command itself are never spoken;
 * a file is named by its base name, as an edit is.
 */
export function commandPhrase(command: string): string {
  const phrases: string[] = [];
  for (const segment of commandSegments(command)) {
    const phrase = segmentPhrase(segment);
    if (phrase && !phrases.includes(phrase)) phrases.push(phrase);
  }
  if (phrases.length === 0) return 'ran a command';
  return phrases.slice(0, 2).join(' and ');
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
