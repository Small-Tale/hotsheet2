import type { ActivityEvent } from './api';

/**
 * What an announced step is about (HS2-RKYCTP), from most to least worth hearing. The editorial
 * policy (`announcer-editorial.ts`) speaks a headline category on its own and folds the rest into a
 * periodic summary; a compound command is named by its most important parts.
 */
export type StepCategory =
  | 'push'
  | 'commit'
  | 'integrate'
  | 'pull-request'
  | 'ticket'
  | 'test'
  | 'check'
  | 'build'
  | 'install'
  | 'milestone'
  | 'edit'
  | 'run'
  | 'web'
  | 'ticket-work'
  | 'tool'
  | 'search'
  | 'read'
  | 'inspect'
  | 'other';

const CATEGORY_ORDER: readonly StepCategory[] = [
  'push',
  'commit',
  'integrate',
  'pull-request',
  'ticket',
  'test',
  'check',
  'build',
  'install',
  'milestone',
  'edit',
  'run',
  'web',
  'ticket-work',
  'tool',
  'search',
  'read',
  'inspect',
  'other',
];
const rank = (category: StepCategory) => CATEGORY_ORDER.indexOf(category);

/** Categories the announcer says on their own, promptly: the plays, not the footwork (HS2-RKYCTP). */
export const HEADLINE_CATEGORIES: ReadonlySet<StepCategory> = new Set<StepCategory>([
  'push',
  'commit',
  'integrate',
  'pull-request',
  'ticket',
  'test',
  'check',
  'build',
  'install',
  'milestone',
]);

/** One thing a step did: a spoken phrase (no agent name, no period) and what it was about. */
export interface StepIntent {
  phrase: string;
  category: StepCategory;
  /** Base names of the files it read or changed. */
  files?: string[];
  /** A plain search word. */
  term?: string;
}

/** An activity event as the announcer understands it: its intent and, for a command, its outcome. */
export interface AnnouncedStep extends StepIntent {
  outcome?: 'passed' | 'failed';
}

type SpeakableEvent = Pick<ActivityEvent, 'tool' | 'kind' | 'summary' | 'detail'>;

/**
 * What the announcer says aloud for an activity event (HS2-R5Q2GY): a short summary of what was done,
 * never a literal command. Commands become a deterministic phrase by what they do; other summaries
 * keep their prose and replace inline code, code blocks, and links with plain words.
 */
export function announcerSpokenText(event: SpeakableEvent): string {
  const tool = agentName(event);
  if (event.kind === 'command') {
    const step = announcerStep(event);
    return `${tool} ${withOutcome(step)}.`;
  }
  // A default tool-call summary names a raw tool id; say what the tool does instead (HS2-NKE7RB).
  if (isDefaultToolCall(event)) return `${tool} ${toolCallPhrase(detailString(event.detail, 'name') ?? '')}.`;
  if (event.kind === 'edit') {
    const path = detailString(event.detail, 'path');
    if (path && /^\S+ edited /.test(event.summary.trim())) return `${tool} edited ${basename(path)}.`;
  }
  const spoken = speakableProse(event.summary);
  return spoken || `${tool} reported ${event.kind.replaceAll('_', ' ')}.`;
}

export const agentName = (event: Pick<ActivityEvent, 'tool'>) => event.tool.trim() || 'The agent';

const isDefaultToolCall = (event: SpeakableEvent) =>
  event.kind === 'tool_call' && /^\S+ (?:called \S+|used a tool)$/.test(event.summary.trim());

/** Kinds that mark a turn's shape or need the person: always worth saying (`docs/15` §15.2). */
const MILESTONE_KINDS = new Set(['turn_end', 'blocked', 'permission', 'decision', 'plan', 'ticket_status']);

/** The intent of any activity event (HS2-RKYCTP): commands and tool calls by what they do, edits by
 * file, turn milestones as milestones, and anything else as `other`. */
export function announcerStep(event: SpeakableEvent): AnnouncedStep {
  if (event.kind === 'command') {
    const intent = commandIntent(commandText(event));
    const outcome = commandOutcome(event.detail);
    return outcome ? { ...intent, outcome } : intent;
  }
  if (event.kind === 'tool_call') {
    // A custom tool-call summary is prose this cannot classify.
    return isDefaultToolCall(event)
      ? toolCallIntent(detailString(event.detail, 'name') ?? '')
      : { phrase: '', category: 'other' };
  }
  if (event.kind === 'edit') {
    const path = detailString(event.detail, 'path');
    const name = path ? basename(path) : undefined;
    return { phrase: name ? `edited ${name}` : 'edited a file', category: 'edit', files: name ? [name] : [] };
  }
  if (MILESTONE_KINDS.has(event.kind)) return { phrase: '', category: 'milestone' };
  if (event.kind === 'note') return { phrase: 'left a note', category: 'other' };
  return { phrase: '', category: 'other' };
}

/** A command's reported outcome: Codex records its exit code; other adapters may record a status. */
export function commandOutcome(detail: unknown): 'passed' | 'failed' | undefined {
  if (detail === null || typeof detail !== 'object' || Array.isArray(detail)) return undefined;
  const fields = detail as Record<string, unknown>;
  const code = fields.exit_code ?? fields.exitCode;
  if (typeof code === 'number' && Number.isFinite(code)) return code === 0 ? 'passed' : 'failed';
  if (fields.status === 'failed' || fields.status === 'error') return 'failed';
  return undefined;
}

/** A phrase with its outcome: test runs say whether they passed; anything else says only a failure. */
export function withOutcome(step: AnnouncedStep): string {
  if (!step.outcome) return step.phrase;
  if (step.category === 'test') return `${step.phrase}, and they ${step.outcome}`;
  return step.outcome === 'failed' ? `${step.phrase}, and it failed` : step.phrase;
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

export const basename = (path: string) =>
  path
    .replace(/[/\\]+$/, '')
    .split(/[/\\]/)
    .pop() ?? path;

/** A command split at unquoted `&&`, `||`, `;`, `&`, `|` and parentheses; `piped` marks a segment that
 * reads a pipe. */
interface CommandSegment {
  words: string[];
  piped: boolean;
}

/** Split a command line into simple commands, keeping quoted text whole and honoring backslash
 * escapes (HS2-RKYCTP). An unterminated quote means the line is unreadable: undefined. */
function commandSegments(text: string): CommandSegment[] | undefined {
  const segments: CommandSegment[] = [];
  let words: string[] = [],
    word = '',
    quoted = false,
    quote: string | undefined,
    piped = false;
  const endWord = () => {
    if (word || quoted) words.push(word);
    word = '';
    quoted = false;
  };
  const endSegment = (nextPiped: boolean) => {
    endWord();
    if (words.length) segments.push({ words, piped });
    words = [];
    piped = nextPiped;
  };
  for (let index = 0; index < text.length; index++) {
    const char = text[index];
    const next = text[index + 1];
    if (quote === "'") {
      if (char === "'") quote = undefined;
      else word += char;
      continue;
    }
    if (quote === '"') {
      if (char === '"') quote = undefined;
      else if (char === '\\' && index + 1 < text.length && '"\\$`'.includes(next)) {
        word += next;
        index++;
      } else word += char;
      continue;
    }
    if (char === '\\') {
      // A line continuation joins lines; any other escaped character is literal.
      if (index + 1 < text.length && next !== '\n') word += next;
      index++;
    } else if (char === '"' || char === "'") {
      quote = char;
      quoted = true;
    } else if (char === '&') {
      if (next === '&') {
        endSegment(false);
        index++;
      } else if (word.endsWith('>') || next === '>') word += char;
      else endSegment(false);
    } else if (char === '|') {
      const double = next === '|';
      endSegment(!double);
      if (double) index++;
    } else if (char === ';' || char === '\n' || char === ')') endSegment(false);
    else if (char === '(') {
      if (word.endsWith('$')) word += char;
      else endSegment(false);
    } else if (/\s/.test(char)) endWord();
    else word += char;
  }
  if (quote) return undefined;
  endSegment(false);
  return segments;
}

/** A word quoted again so a rebuilt script tokenizes the same way. */
const shellQuote = (word: string) => (/^[\w@%+=:,./-]+$/.test(word) ? word : `'${word.replaceAll("'", `'\\''`)}'`);

/** Leading flags (and the values of `valued` flags) dropped from `words`. */
function skipFlags(words: string[], valued: readonly string[] = []): string[] {
  const rest = [...words];
  while (rest.length && rest[0].startsWith('-') && rest[0] !== '-') {
    const flag = rest.shift() ?? '';
    if (flag === '--') break;
    if (valued.includes(flag)) rest.shift();
  }
  return rest;
}

const SHELLS = new Set(['sh', 'bash', 'zsh', 'dash', 'ksh', 'fish']);
const PLAIN_WRAPPERS = new Set(['time', 'nohup', 'exec', 'command', 'builtin', 'caffeinate', 'stdbuf']);
const PACKAGE_RUNNERS = new Set(['npx', 'bunx', 'pnpx', 'uvx']);

/**
 * A simple command with its wrappers removed (HS2-RKYCTP): env assignments, `env`, `sudo`, `time`,
 * `nohup`, `nice`, `timeout N`, `xargs`, `npx`/`npm exec`/`pnpm dlx`-style package runners, and
 * `uv run`-style environment runners. A shell (`zsh -lc '…'`, `bash -c …`, `eval …`) yields the script
 * it runs instead, whether the script arrives quoted or, as Codex's argv joins it, unquoted.
 */
function unwrapCommand(input: readonly string[]): { words: string[]; script?: string } {
  let words = [...input];
  for (;;) {
    const [first = '', second = ''] = words;
    const program = basename(first).toLowerCase();
    if (/^[A-Za-z_]\w*=/.test(first)) words = words.slice(1);
    else if (program === 'sudo' || program === 'doas') words = skipFlags(words.slice(1), ['-u', '-g', '-C']);
    else if (program === 'env') {
      words = skipFlags(words.slice(1), ['-u', '-C', '-S']);
      while (words.length && /^[A-Za-z_]\w*=/.test(words[0])) words = words.slice(1);
    } else if (PLAIN_WRAPPERS.has(program)) words = skipFlags(words.slice(1));
    else if (program === 'nice') words = skipFlags(words.slice(1), ['-n']);
    else if (program === 'timeout' || program === 'gtimeout')
      words = skipFlags(words.slice(1), ['-s', '-k', '--signal', '--kill-after']).slice(1);
    else if (program === 'xargs') words = skipFlags(words.slice(1), ['-n', '-I', '-P', '-L', '-d', '-s', '-E', '-a']);
    else if (PACKAGE_RUNNERS.has(program)) words = skipFlags(words.slice(1), ['-p', '--package']);
    else if (/^(?:npm|pnpm|yarn|bun)$/.test(program) && /^(?:exec|dlx|x)$/.test(second))
      words = skipFlags(words.slice(2), ['-c', '--call', '-p', '--package']);
    else if (
      (/^(?:uv|poetry|pipenv|pdm|rye|hatch)$/.test(program) && second === 'run') ||
      (program === 'bundle' && second === 'exec')
    )
      words = skipFlags(words.slice(2), ['--with', '--python', '-p']);
    else if (program === 'eval') return { words: [], script: words.slice(1).join(' ') };
    else if (SHELLS.has(program)) {
      let index = 1;
      while (index < words.length && /^[-+]/.test(words[index])) {
        const flag = words[index];
        // `-c`, `-lc`, `-ilc`, `-ec`…: the next argument is the script.
        if (/^-[A-Za-z]*c[A-Za-z]*$/.test(flag)) {
          const rest = words.slice(index + 1);
          return { words: [], script: rest.length === 1 ? rest[0] : rest.map(shellQuote).join(' ') };
        }
        index += flag === '-o' || flag === '+o' ? 2 : 1;
      }
      // `bash script.sh` runs that script; a bare shell says nothing.
      words = words.slice(index);
      if (!words.length || SHELLS.has(basename(words[0]).toLowerCase())) return { words: [] };
      return { words };
    } else return { words };
  }
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
  'trap',
  'shopt',
  'ulimit',
  'umask',
  'test',
  '[',
  '[[',
  'exit',
  'return',
  'local',
  'declare',
  'read',
  'which',
  'type',
  'pwd',
  'date',
  'mktemp',
  'dirname',
  'basename',
  'realpath',
]);

const intent = (phrase: string, category: StepCategory, extra: Partial<StepIntent> = {}): StepIntent => ({
  phrase,
  category,
  ...extra,
});

function gitIntent(args: readonly string[]): StepIntent {
  // `git -C <dir> …` names its directory before the subcommand.
  const rest = [...args];
  while (rest.length && rest[0].startsWith('-')) rest.splice(0, rest[0] === '-C' || rest[0] === '-c' ? 2 : 1);
  const [sub, ...more] = rest,
    targets = positional(more);
  switch (sub) {
    case 'commit':
      return intent('committed changes', 'commit');
    case 'push': {
      const branch = targets.at(-1)?.split(':').pop();
      return targets.length > 1 && speakableWord(branch)
        ? intent(`pushed changes to ${branch}`, 'push')
        : intent('pushed changes', 'push');
    }
    case 'rebase':
    case 'merge':
    case 'cherry-pick':
      return intent('integrated changes', 'integrate');
    case 'pull':
    case 'fetch':
      return intent('updated from the remote', 'run');
    case 'status':
      return intent('checked what changed', 'inspect');
    case 'diff':
      return more.includes('--cached') || more.includes('--staged')
        ? intent('reviewed the staged changes', 'inspect')
        : intent('reviewed the changes', 'inspect');
    case 'log':
      return intent('read the commit history', 'inspect');
    case 'show':
      return intent('looked at a commit', 'inspect');
    case 'blame':
      return intent('checked who changed the code', 'inspect');
    case 'branch':
      return intent('checked the branches', 'inspect');
    case 'add':
      return intent('staged changes', 'run');
    case 'checkout':
    case 'switch':
      return intent('switched branches', 'run');
    case 'worktree':
      return intent('managed worktrees', 'run');
    case 'stash':
      return intent('stashed changes', 'run');
    case 'reset':
    case 'restore':
      return intent('reset changes', 'run');
    case 'clone':
      return intent('cloned a repository', 'run');
    default:
      return intent('used Git', 'run');
  }
}

function hotSheetIntent(args: readonly string[]): StepIntent {
  const rest = [...args];
  // `-C <store>` comes before the subcommand.
  while (rest.length && rest[0].startsWith('-')) rest.splice(0, rest[0] === '-C' || rest[0] === '--path' ? 2 : 1);
  const [sub, ...more] = rest;
  const status = more.includes('--status') ? more[more.indexOf('--status') + 1] : undefined;
  switch (sub) {
    case 'claim':
    case 'claim-next':
      return intent('claimed a ticket', 'ticket');
    case 'release':
      return intent('released a ticket', 'ticket');
    case 'new':
      return intent('created a ticket', 'ticket');
    case 'renew':
      return intent('renewed a ticket claim', 'ticket-work');
    case 'show':
      return intent('read a ticket', 'ticket-work');
    case 'ls':
      return intent('listed tickets', 'ticket-work');
    case 'attach':
      return intent('attached files to a ticket', 'ticket-work');
    case 'edit':
      if (status === 'completed') return intent('completed a ticket', 'ticket');
      if (status) return intent('changed a ticket status', 'ticket');
      if (more.includes('--note') || more.includes('--note-file')) return intent('added a ticket note', 'ticket-work');
      return intent('updated a ticket', 'ticket-work');
    default:
      return intent('updated Hot Sheet tickets', 'ticket-work');
  }
}

const TEST_RUNNERS: ReadonlyArray<[RegExp, string]> = [
  [/^(?:nextest)$/, 'ran the Rust tests'],
  [/^playwright$/, 'ran the browser tests'],
  [/^(?:vitest|jest|mocha)$/, 'ran the unit tests'],
  [/^pytest$/, 'ran the Python tests'],
  [/^check\.sh$/, 'ran the project checks'],
];

const SCRIPT_FILE = /\.(?:sh|bash|zsh|mjs|cjs|js|ts|mts|py|rb)$/;

/** A script by what its name says it does (`scripts/test-quiet.mjs` runs tests). */
function scriptIntent(path: string, fallback = 'ran a script'): StepIntent {
  const stem = basename(path).replace(SCRIPT_FILE, '').toLowerCase();
  if (/(?:^|[-_.])tests?(?:$|[-_.])/.test(stem)) return intent('ran the tests', 'test');
  if (/(?:^|[-_.])(?:check|verify|gate)(?:$|[-_.])/.test(stem)) return intent('ran the project checks', 'test');
  if (/(?:^|[-_.])lint(?:$|[-_.])/.test(stem)) return intent('linted the code', 'check');
  if (/(?:^|[-_.])build(?:$|[-_.])/.test(stem)) return intent('built the project', 'build');
  if (/(?:^|[-_.])push(?:$|[-_.])/.test(stem)) return intent('pushed changes', 'push');
  const spoken = stem.replace(/[-_.]+/g, ' ');
  return speakableWord(spoken, 32) ? intent(`ran the ${spoken} script`, 'run') : intent(fallback, 'run');
}

/** A task-runner target (`make test`, `just lint`) by what it names. */
function targetIntent(program: string, target: string | undefined): StepIntent {
  if (target && /^tests?$/.test(target)) return intent('ran the tests', 'test');
  if (target && /^(?:check|verify)$/.test(target)) return intent('ran the project checks', 'test');
  if (target === 'lint') return intent('linted the code', 'check');
  if (target === 'build' || target === 'all') return intent('built the project', 'build');
  if (target === 'install') return intent('installed dependencies', 'install');
  return speakableWord(target) ? intent(`ran the ${target} task`, 'run') : intent(`ran ${program}`, 'run');
}

/** The intent of one simple command, or undefined when it says nothing about the work. */
function segmentIntent(segment: CommandSegment, words: readonly string[]): StepIntent | undefined {
  const program = basename(words[0] ?? '').toLowerCase();
  const args = words.slice(1);
  const lower = args.map((word) => word.toLowerCase());
  const has = (...names: string[]) => names.some((name) => lower.includes(name));
  if (!program || NOISE.has(program)) return undefined;
  const inPlace = /^(?:sed|perl)$/.test(program) && args.some((arg) => /^-[A-Za-z]*i/.test(arg));
  if (segment.piped && PIPE_FILTERS.has(program) && !inPlace && !args.some((arg) => fileName(arg))) return undefined;
  if (program === 'git') return gitIntent(args);
  if (/hotsheet/.test(program)) return hotSheetIntent(args);
  if (program === 'gh') {
    if (has('pr')) {
      if (has('create')) return intent('opened a pull request', 'pull-request');
      if (has('merge')) return intent('merged a pull request', 'pull-request');
      return intent('checked a pull request', 'inspect');
    }
    if (has('run', 'workflow')) return intent('checked CI', 'inspect');
    if (has('issue')) return intent('worked on an issue', 'run');
    return intent('used GitHub', 'run');
  }
  if (/^(?:curl|wget|http)$/.test(program)) return intent('made a network request', 'web');
  const tokens = [program, ...lower.map((arg) => basename(arg))];
  for (const [pattern, phrase] of TEST_RUNNERS)
    if (tokens.some((token) => pattern.test(token))) return intent(phrase, 'test');
  if (program === 'cargo') {
    if (has('test')) return intent('ran the Rust tests', 'test');
    if (has('clippy')) return intent('linted the Rust code', 'check');
    if (has('fmt')) return intent('checked the formatting', 'check');
    if (has('build', 'check')) return intent('built the Rust code', 'build');
    if (has('run')) return intent('ran the Rust program', 'run');
    if (has('add', 'update', 'fetch')) return intent('installed dependencies', 'install');
  }
  // npm/pnpm/yarn scripts: `test`, `lint`, `build`, or the script's own plain name.
  if (/^(?:npm|pnpm|yarn|bun)$/.test(program)) {
    const verb = lower.at(0) ?? '',
      script = verb === 'run' || verb === 'run-script' ? (lower.at(1) ?? '') : verb;
    if (/^(?:install|i|ci|add)$/.test(verb)) return intent('installed dependencies', 'install');
    if (/^test(?::|$)/.test(script)) return intent('ran the tests', 'test');
    if (/^lint(?::|$)/.test(script)) return intent('linted the code', 'check');
    if (/^(?:format|fmt)(?::|$)/.test(script)) return intent('checked the formatting', 'check');
    if (/^typecheck(?::|$)/.test(script)) return intent('type-checked the code', 'check');
    if (/^build(?::|$)/.test(script)) return intent('built the project', 'build');
    return speakableWord(script) ? intent(`ran the ${script} script`, 'run') : intent('ran a project script', 'run');
  }
  if (/^(?:make|just|task|rake)$/.test(program)) return targetIntent(program, positional(args)[0]);
  if (/^(?:pip|pip3|brew)$/.test(program) && /^(?:install|add)$/.test(lower[0] ?? ''))
    return intent('installed dependencies', 'install');
  if (program === 'eslint') return intent('linted the code', 'check');
  if (/^(?:prettier|rustfmt)$/.test(program))
    return has('--write', '-w') ? intent('formatted the code', 'edit') : intent('checked the formatting', 'check');
  if (program === 'tsc') return intent('type-checked the code', 'check');
  if (/^(?:rg|grep|ag|ack)$/.test(program)) {
    if (has('--files', '-l', '--files-with-matches')) return intent('looked for files', 'search');
    const pattern = positional(args)[0];
    return speakableWord(pattern)
      ? intent(`searched the code for ${pattern}`, 'search', { term: pattern })
      : intent('searched the code', 'search');
  }
  if (/^(?:find|fd)$/.test(program)) return intent('looked for files', 'search');
  const files = args.map(fileName).filter((name): name is string => Boolean(name));
  const redirect = args.findIndex((arg) => arg === '>' || arg === '>>' || /^>>?[^&]/.test(arg));
  if (redirect >= 0 || program === 'tee') {
    const target = program === 'tee' ? positional(args)[0] : args[redirect].replace(/^>>?/, '') || args[redirect + 1];
    const name = target ? fileName(target) : undefined;
    return name ? intent(`wrote ${name}`, 'edit', { files: [name] }) : intent('wrote a file', 'edit');
  }
  if (inPlace) {
    return files.length === 1
      ? intent(`changed ${files[0]}`, 'edit', { files })
      : intent('changed files', 'edit', { files });
  }
  if (/^(?:cat|sed|head|tail|less|nl|bat|more|awk)$/.test(program)) {
    if (files.length === 1) return intent(`read ${files[0]}`, 'read', { files });
    return files.length > 1 ? intent(`read ${files.length} files`, 'read', { files }) : intent('read files', 'read');
  }
  if (/^(?:ls|tree|exa|eza|stat|file)$/.test(program)) return intent('listed files', 'inspect');
  if (program === 'wc') return intent('counted lines', 'inspect');
  if (program === 'mkdir') return intent('created a folder', 'edit');
  if (program === 'touch')
    return files.length === 1 ? intent(`created ${files[0]}`, 'edit', { files }) : intent('created a file', 'edit');
  if (program === 'rm')
    return files.length === 1 ? intent(`deleted ${files[0]}`, 'edit', { files }) : intent('deleted files', 'edit');
  if (program === 'mv')
    return files.length >= 1
      ? intent(`moved ${files[0]}`, 'edit', { files: [files[0]] })
      : intent('moved files', 'edit');
  if (program === 'cp')
    return files.length >= 1
      ? intent(`copied ${files[0]}`, 'edit', { files: [files[0]] })
      : intent('copied files', 'edit');
  if (/^(?:ln|chmod|chown)$/.test(program)) return intent('changed file settings', 'edit');
  // An interpreter running a script file is named by that script.
  if (/^(?:python|python3|node|tsx|ts-node|deno|bun|ruby|perl)$/.test(program)) {
    const script = positional(args).find((arg) => SCRIPT_FILE.test(arg) && fileName(arg));
    const fallback = /^python/.test(program) ? 'ran a Python script' : 'ran a Node script';
    return script ? scriptIntent(script, fallback) : intent(fallback, 'run');
  }
  if (/^(?:kill|pkill|killall)$/.test(program)) return intent('stopped a process', 'run');
  if (/^(?:ps|lsof|pgrep|top)$/.test(program)) return intent('checked running processes', 'inspect');
  if (/^(?:df|du)$/.test(program)) return intent('checked disk space', 'inspect');
  if (program === 'jq') return intent('read JSON', 'read');
  if (program === 'open') return intent('opened a file', 'run');
  if (SCRIPT_FILE.test(program)) return scriptIntent(program);
  return speakableWord(program, 20) ? intent(`ran ${program}`, 'run') : undefined;
}

/** Every intent of a command line, in order, with shells and wrappers unwrapped (HS2-RKYCTP). */
function commandIntents(command: string, depth = 0): StepIntent[] {
  const intents: StepIntent[] = [];
  for (const segment of commandSegments(command.trim()) ?? []) {
    const unwrapped = unwrapCommand(segment.words);
    if (unwrapped.script !== undefined) {
      if (depth < 4) intents.push(...commandIntents(unwrapped.script, depth + 1));
      continue;
    }
    const found = segmentIntent(segment, unwrapped.words);
    if (found) intents.push(found);
  }
  return intents;
}

/**
 * What a command does (HS2-R5Q2GY, HS2-RKYCTP): shells (`zsh -lc '…'`) and wrappers (`env`, `timeout`,
 * `npx`…) are unwrapped, each simple command of a compound line is named by what it does,
 * housekeeping (`cd`, `echo`, pipe filters) is dropped, and the two most important distinct phrases
 * are said in the order they ran. Its category is the most important part's. Paths, flags, quoted
 * arguments, and the command itself are never spoken; a file is named by its base name, as an edit is.
 */
export function commandIntent(command: string): StepIntent {
  const distinct: StepIntent[] = [];
  for (const found of commandIntents(command))
    if (!distinct.some((entry) => entry.phrase === found.phrase)) distinct.push(found);
  if (distinct.length === 0) return intent('ran a command', 'other');
  const chosen = [...distinct].sort((left, right) => rank(left.category) - rank(right.category)).slice(0, 2);
  const spoken = distinct.filter((entry) => chosen.includes(entry));
  const files = [...new Set(distinct.flatMap((entry) => entry.files ?? []))];
  const term = distinct.find((entry) => entry.term)?.term;
  return {
    phrase: spoken.map((entry) => entry.phrase).join(' and '),
    category: chosen[0].category,
    ...(files.length ? { files } : {}),
    ...(term ? { term } : {}),
  };
}

/** The spoken phrase for a command; see {@link commandIntent}. */
export const commandPhrase = (command: string): string => commandIntent(command).phrase;

/** Hot Sheet's own MCP tools by what they do to tickets. */
const HOT_SHEET_TOOLS: ReadonlyArray<[RegExp, string, StepCategory]> = [
  [/^(?:query|ls|list)$/, 'listed tickets', 'ticket-work'],
  [/^(?:get|show)$/, 'read a ticket', 'ticket-work'],
  [/^(?:update|edit|batch)$/, 'updated a ticket', 'ticket-work'],
  [/^(?:claim|claim_next)$/, 'claimed a ticket', 'ticket'],
  [/^release$/, 'released a ticket', 'ticket'],
  [/^renew$/, 'renewed a ticket claim', 'ticket-work'],
  [/^(?:create|new)$/, 'created a ticket', 'ticket'],
  [/^close$/, 'closed a ticket', 'ticket'],
  [/^targets$/, 'worked on orchestration targets', 'ticket-work'],
];

/** Built-in agent tools (Claude Code's and Codex's) by what they do. */
const BUILT_IN_TOOLS: Readonly<Record<string, [string, StepCategory]>> = {
  read: ['read a file', 'read'],
  view: ['read a file', 'read'],
  grep: ['searched the code', 'search'],
  search: ['searched the code', 'search'],
  glob: ['looked for files', 'search'],
  ls: ['listed files', 'inspect'],
  webfetch: ['read a web page', 'web'],
  websearch: ['searched the web', 'web'],
  'web search': ['searched the web', 'web'],
  task: ['started a sub-agent', 'milestone'],
  agent: ['started a sub-agent', 'milestone'],
  todowrite: ['updated its task list', 'other'],
  askuserquestion: ['asked you a question', 'milestone'],
  exitplanmode: ['finished planning', 'milestone'],
  'image view': ['looked at an image', 'read'],
};

/** A tool or server name as words a voice can say (`code_review` → `code review`), or undefined. */
function toolWords(name: string): string | undefined {
  const words = name
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/[_.-]+/g, ' ')
    .trim()
    .toLowerCase();
  return /^[a-z][a-z ]{1,30}$/.test(words) ? words : undefined;
}

/**
 * What a tool call does (HS2-NKE7RB): Hot Sheet's ticket tools by their ticket action, the agent's
 * built-in tools by what they do, another MCP server by its name, and anything else by its plain-word
 * name. Arguments are never spoken; an unspeakable name is "used a tool".
 */
export function toolCallIntent(name: string): StepIntent {
  const trimmed = name.trim();
  // `mcp__server__tool` (Claude) or `server.tool` (Codex).
  const mcp = /^mcp__([^_]+(?:_[^_]+)*?)__(.+)$/.exec(trimmed) ?? /^([\w-]+)\.([\w-]+)$/.exec(trimmed);
  const server = mcp?.[1],
    tool = mcp?.[2] ?? trimmed;
  if (server === 'hotsheet' || /^hotsheet_/.test(tool)) {
    const action = tool.replace(/^hotsheet_/, '');
    for (const [pattern, phrase, category] of HOT_SHEET_TOOLS)
      if (pattern.test(action)) return intent(phrase, category);
    return intent('used Hot Sheet', 'ticket-work');
  }
  if (server) {
    const words = toolWords(server);
    return intent(words ? `used ${words} tools` : 'used a tool', 'tool');
  }
  const builtIn = BUILT_IN_TOOLS[trimmed.toLowerCase()] as [string, StepCategory] | undefined;
  if (builtIn) return intent(builtIn[0], builtIn[1]);
  const words = toolWords(trimmed);
  return intent(words ? `used ${words}` : 'used a tool', 'tool');
}

/** The spoken phrase for a tool call; see {@link toolCallIntent}. */
export const toolCallPhrase = (name: string): string => toolCallIntent(name).phrase;

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
