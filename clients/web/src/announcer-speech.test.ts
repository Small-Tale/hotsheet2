import { describe, expect, it } from 'vitest';

import { announcerItem } from './announcer-reel';
import {
  announcerSpokenText,
  announcerStep,
  commandIntent,
  commandOutcome,
  commandPhrase,
  firstPerson,
  speakableProse,
  toolCallIntent,
  toolCallPhrase,
} from './announcer-speech';

const command = (text: string, summary = `claude ran \`${text}\``) =>
  announcerSpokenText({ tool: 'claude', kind: 'command', summary, detail: { command: text } });

describe('announcer speech never reads a literal command (HS2-R5Q2GY)', () => {
  const commands = [
    "cat > $TMPDIR/ft.md <<'EOF' ## Result Kerf bumped to 5.0.0-beta.122",
    '/bin/zsh -lc "sed -n \'1,240p\' CLAUDE.md && rg -n --hidden overlay ."',
    '/bin/zsh -lc \'rg --files -g "!node_modules" | head -250\'',
    'cargo nextest run -E "test(synthetic)"',
    'git commit -m "HS2-XXXXXX Synthetic subject"',
    'npm ci --no-audit',
    'NODE_ENV=test npx vitest run src/sample.test.ts',
    'python3 -I scripts/synthetic.py --flag',
  ];

  it.each(commands)('summarizes %s without its text', (text) => {
    const spoken = command(text);
    // Words, and at most a bare file name such as `notes.md` (HS2-RKYCTP), never code.
    expect(spoken).toMatch(/^I [a-z]+(?: [\w.-]+)*\.$/i);
    expect(spoken).not.toContain('`');
    expect(spoken).not.toContain(text);
    // No path, flag, quoted argument, variable, or other code fragment of the command is spoken.
    for (const fragment of text.split(/\s+/).filter((word) => /[/'"$=-]/.test(word) && word.length > 1))
      expect(spoken).not.toContain(fragment);
    expect(spoken).not.toMatch(/[/'"$=]| -/);
  });

  it('falls back to the summary code span when the detail has no command', () => {
    expect(
      announcerSpokenText({
        tool: 'codex',
        kind: 'command',
        summary: 'codex ran `git push origin main`',
        detail: null,
      }),
    ).toBe('I pushed changes to main.');
    expect(announcerSpokenText({ tool: 'codex', kind: 'command', summary: 'codex ran a command', detail: {} })).toBe(
      'I ran a command.',
    );
  });

  it('names what common commands do', () => {
    expect(commandPhrase('git status --short')).toBe('checked what changed');
    expect(commandPhrase('git diff --cached')).toBe('reviewed the staged changes');
    expect(commandPhrase('git -C ../repo log --oneline -5')).toBe('read the commit history');
    expect(commandPhrase('git commit -m x')).toBe('committed changes');
    expect(commandPhrase('git push')).toBe('pushed changes');
    expect(commandPhrase('git pull --rebase')).toBe('updated from the remote');
    expect(commandPhrase('git rebase origin/main')).toBe('integrated changes');
    expect(commandPhrase('git stash list')).toBe('stashed changes');
    expect(commandPhrase('git bisect start')).toBe('used Git');
    expect(commandPhrase('scripts/check.sh --plan')).toBe('ran the project checks');
    expect(commandPhrase('npx vitest run')).toBe('ran the unit tests');
    expect(commandPhrase('npx playwright test tests/a.spec.ts')).toBe('ran the browser tests');
    expect(commandPhrase('cargo nextest run -p sample')).toBe('ran the Rust tests');
    expect(commandPhrase('cargo test')).toBe('ran the Rust tests');
    expect(commandPhrase('cargo clippy --workspace')).toBe('linted the Rust code');
    expect(commandPhrase('npm run lint')).toBe('linted the code');
    expect(commandPhrase('npm run catalog:generate')).toBe('ran a project script');
    expect(commandPhrase('npm run dev')).toBe('ran the dev script');
    expect(commandPhrase('cargo build --release')).toBe('built the Rust code');
    expect(commandPhrase('npx tsc --noEmit')).toBe('type-checked the code');
    expect(commandPhrase('npm install')).toBe('installed dependencies');
    expect(commandPhrase('rg -n sample src')).toBe('searched the code for sample');
    expect(commandPhrase('rg -n "a|b(c)" src')).toBe('searched the code');
    expect(commandPhrase('cat > notes.md')).toBe('wrote notes.md');
    expect(commandPhrase('sed -n 1,20p src/a.ts')).toBe('read a.ts');
    expect(commandPhrase('head -40 a.ts b.ts')).toBe('read 2 files');
    expect(commandPhrase('mkdir -p out')).toBe('created a folder');
    expect(commandPhrase('rm -f build/out.log')).toBe('deleted out.log');
    expect(commandPhrase('curl -s https://example.test')).toBe('made a network request');
    expect(commandPhrase('gh run view 123')).toBe('checked CI');
    expect(commandPhrase('hotsheet-cli ls --up-next')).toBe('listed tickets');
    expect(commandPhrase('python3 -I scripts/x.py')).toBe('ran a Python script');
    expect(commandPhrase('./scripts/sync-docs.sh')).toBe('ran the sync docs script');
    expect(commandPhrase('sudo env A=1 make')).toBe('ran make');
    expect(commandPhrase('')).toBe('ran a command');
    expect(commandPhrase('"unterminated')).toBe('ran a command');
  });

  it('is specific about compound commands and ticket work (HS2-RKYCTP)', () => {
    // Housekeeping and pipe filters say nothing; the work does.
    expect(commandPhrase('cd clients/web && npx vitest run src/a.test.ts')).toBe('ran the unit tests');
    expect(commandPhrase("rg --files -g '!node_modules' | head -250")).toBe('looked for files');
    expect(commandPhrase('git log --oneline | head -5')).toBe('read the commit history');
    expect(commandPhrase('npm ci && npm run build; echo done')).toBe('installed dependencies and built the project');
    // At most two distinct phrases, the most important ones, in the order they ran.
    expect(commandPhrase('git add a.ts && git commit -m x && git push origin main')).toBe(
      'committed changes and pushed changes to main',
    );
    expect(commandPhrase('cargo test || cargo test')).toBe('ran the Rust tests');
    expect(commandPhrase('cd x; export A=1')).toBe('ran a command');
    // A separator inside quotes is part of the argument.
    expect(commandPhrase('git commit -m "a && b | c"')).toBe('committed changes');
    // Ticket actions by what they do.
    expect(commandPhrase('hotsheet-cli -C ../store claim HS2-AAAAAA --worker w')).toBe('claimed a ticket');
    expect(commandPhrase('hotsheet-cli edit HS2-AAAAAA --status completed --note-file n.md')).toBe(
      'completed a ticket',
    );
    expect(commandPhrase('hotsheet-cli edit HS2-AAAAAA --note x')).toBe('added a ticket note');
    expect(commandPhrase('hotsheet-cli edit HS2-AAAAAA --priority high')).toBe('updated a ticket');
    expect(commandPhrase('hotsheet-cli new --title x')).toBe('created a ticket');
    expect(commandPhrase('hotsheet-cli frobnicate')).toBe('updated Hot Sheet tickets');
    expect(
      announcerSpokenText({
        tool: 'claude',
        kind: 'command',
        summary: 'claude ran a command',
        detail: { command: "/bin/zsh -lc 'cd /work/repo && git diff -- src | head -80'" },
      }),
    ).toBe('I reviewed the changes.');
  });
});

describe('announcer speech for other events (HS2-R5Q2GY)', () => {
  it('keeps prose and replaces code, blocks, and links with plain words', () => {
    expect(speakableProse('codex decided: run `cargo test -p sample` before `kerfjs` bumps')).toBe(
      'codex decided: run a command before kerfjs bumps',
    );
    expect(speakableProse('Updated `docs/15-activity.md` and **pushed**')).toBe('Updated 15-activity.md and pushed');
    expect(speakableProse('## Result\n```sh\nrm -rf build\n```\nSee https://example.test/x')).toBe('Result See a link');
    expect(speakableProse('stray ` backtick')).toBe('stray backtick');
  });

  it('says an edit by file name and keeps a custom edit summary', () => {
    expect(
      announcerSpokenText({
        tool: 'claude',
        kind: 'edit',
        summary: 'claude edited clients/web/src/sample.ts',
        detail: { path: 'clients/web/src/sample.ts' },
      }),
    ).toBe('I edited sample.ts.');
    expect(
      announcerSpokenText({
        tool: 'codex',
        kind: 'edit',
        summary: 'Updated the synthetic sample file',
        detail: { path: 'src/sample.ts' },
      }),
    ).toBe('Updated the synthetic sample file');
  });

  it('names an empty event by its kind', () => {
    expect(announcerSpokenText({ tool: 'codex', kind: 'turn_end', summary: '``', detail: null })).toBe(
      'I reported turn end.',
    );
    expect(announcerSpokenText({ tool: ' ', kind: 'command', summary: '', detail: null })).toBe('I ran a command.');
  });

  it('gives every reel item display text and separate spoken text', () => {
    const item = announcerItem('demo', 'Demo', 'store', {
      id: 'x',
      ts: '2026-11-01T00:00:00Z',
      tool: 'claude',
      kind: 'command',
      summary: 'claude ran `cat > notes.md`',
      detail: { command: 'cat > notes.md' },
      importance: 'normal',
    });
    expect(item.narration).toBe('claude ran `cat > notes.md`');
    expect(item.spoken).toBe('I wrote notes.md.');
  });
});

describe('announcer speech for tool calls (HS2-NKE7RB)', () => {
  it('names what a tool does instead of its raw id', () => {
    expect(toolCallPhrase('mcp__hotsheet__hotsheet_update')).toBe('updated a ticket');
    expect(toolCallPhrase('mcp__hotsheet__hotsheet_claim_next')).toBe('claimed a ticket');
    expect(toolCallPhrase('hotsheet.hotsheet_query')).toBe('listed tickets');
    expect(toolCallPhrase('hotsheet.hotsheet_targets')).toBe('worked on orchestration targets');
    expect(toolCallPhrase('mcp__hotsheet__hotsheet_annotate_attachment')).toBe('used Hot Sheet');
    expect(toolCallPhrase('mcp__claude_in_chrome__navigate')).toBe('used claude in chrome tools');
    expect(toolCallPhrase('github.create_issue')).toBe('used github tools');
    expect(toolCallPhrase('Read')).toBe('read a file');
    expect(toolCallPhrase('Grep')).toBe('searched the code');
    expect(toolCallPhrase('WebFetch')).toBe('read a web page');
    expect(toolCallPhrase('web search')).toBe('searched the web');
    expect(toolCallPhrase('Task')).toBe('started a sub-agent');
    expect(toolCallPhrase('AskUserQuestion')).toBe('asked you a question');
    expect(toolCallPhrase('NotebookRead')).toBe('used notebook read');
    expect(toolCallPhrase('')).toBe('used a tool');
    expect(toolCallPhrase('x/../../etc')).toBe('used a tool');
  });

  it('replaces only the default summary and keeps custom tool-call prose', () => {
    const spoken = (summary: string, name?: string) =>
      announcerSpokenText({ tool: 'claude', kind: 'tool_call', summary, detail: name ? { name } : null });
    expect(spoken('claude called mcp__hotsheet__hotsheet_get', 'mcp__hotsheet__hotsheet_get')).toBe('I read a ticket.');
    expect(spoken('claude used a tool')).toBe('I used a tool.');
    expect(spoken('Fetched the release notes', 'WebFetch')).toBe('Fetched the release notes');
  });
});

describe('announcer speech unwraps shells and wrappers (HS2-RKYCTP)', () => {
  // Codex runs every command through `zsh -lc`; its rollout joined the argv with spaces, so stored
  // events carry the script unquoted (`codex_rollout_watch.rs`), while the app server quotes it.
  const unwrapped: [string, string][] = [
    ["/bin/zsh -lc 'cd /work/repo && npm test'", 'ran the tests'],
    ['/bin/zsh -lc "cd /work/repo && npm test"', 'ran the tests'],
    ['/bin/zsh -lc cd /work/repo && npm test', 'ran the tests'],
    ['/bin/zsh -lc rg -n overlay src', 'searched the code for overlay'],
    ["zsh -c 'git status --short'", 'checked what changed'],
    ["bash -lc 'cargo nextest run -p sample'", 'ran the Rust tests'],
    ["bash -ilc 'git push origin main'", 'pushed changes to main'],
    ["/usr/bin/bash -euc 'npm ci && npm run build'", 'installed dependencies and built the project'],
    ['sh -c \'git commit -m "a && b"\'', 'committed changes'],
    ["bash -o pipefail -c 'npx vitest run | tail -5'", 'ran the unit tests'],
    ['bash -lc "bash -c \\"npm run lint\\""', 'linted the code'],
    ['zsh -lc "bash -c \'git log --oneline | head -5\'"', 'read the commit history'],
    ["/bin/zsh -lc 'set -euo pipefail; cd x; hotsheet-cli claim HS2-AAAAAA --worker w'", 'claimed a ticket'],
    ["eval 'git diff --cached'", 'reviewed the staged changes'],
    ['bash scripts/sync-docs.sh', 'ran the sync docs script'],
    ['env -u HOME FOO=1 BAR=2 cargo test', 'ran the Rust tests'],
    ['FOO=1 BAR="a b" npm run lint', 'linted the code'],
    ['timeout 120 npx playwright test tests/a.spec.ts', 'ran the browser tests'],
    ['timeout -s KILL 5m cargo clippy', 'linted the Rust code'],
    ['nice -n 10 nohup cargo build --release', 'built the Rust code'],
    ['sudo -u builder make test', 'ran the tests'],
    ['npx --yes prettier --check src', 'checked the formatting'],
    ['npm exec -- tsc --noEmit', 'type-checked the code'],
    ['pnpm dlx eslint .', 'linted the code'],
    ['bunx vitest run', 'ran the unit tests'],
    ['uv run pytest -q', 'ran the Python tests'],
    ['node scripts/test-quiet.mjs', 'ran the tests'],
    ['node scripts/sync-ai-playbook.mjs --check', 'ran the sync ai playbook script'],
    ["rg -l TODO src | xargs sed -i '' 's/a/b/'", 'looked for files and changed files'],
    ['(cd clients/web && npm test)', 'ran the tests'],
    ['npm test 2>&1 | tail -20', 'ran the tests'],
    ['cargo test \\\n  -p sample', 'ran the Rust tests'],
    ['zsh -lc', 'ran a command'],
    ['bash', 'ran a command'],
    ["zsh -lc 'unterminated", 'ran a command'],
  ];
  it.each(unwrapped)('%s → %s', (text, phrase) => {
    expect(commandPhrase(text)).toBe(phrase);
  });

  it('never names the wrapper shell (the "codex ran zsh" report)', () => {
    for (const shell of ['/bin/zsh', 'zsh', '/bin/bash', 'bash', 'sh'])
      for (const flag of ['-c', '-lc', '-ilc'])
        expect(commandPhrase(`${shell} ${flag} ./scripts/check.sh --plan`)).toBe('ran the project checks');
    expect(commandPhrase('/bin/zsh -lc python3 - <<PY')).toBe('ran a Python script');
  });

  it('carries a category so the editorial policy can tell plays from footwork', () => {
    expect(commandIntent('/bin/zsh -lc cd x && git add a && git commit -m y')).toMatchObject({
      phrase: 'staged changes and committed changes',
      category: 'commit',
    });
    expect(commandIntent('sed -n 1,20p src/a.ts && rg -n overlay')).toMatchObject({
      category: 'search',
      files: ['a.ts'],
      term: 'overlay',
    });
    expect(commandIntent('cat > notes.md').category).toBe('edit');
    expect(commandIntent('npx vitest run').category).toBe('test');
    expect(commandIntent('ls -la').category).toBe('inspect');
    expect(commandIntent('gh pr create --fill').category).toBe('pull-request');
    expect(commandIntent('hotsheet-cli edit HS2-AAAAAA --status started').phrase).toBe('changed a ticket status');
    expect(commandIntent('').category).toBe('other');
  });
});

describe('announcer speech says how a command ended (HS2-RKYCTP)', () => {
  const ran = (text: string, detail: Record<string, unknown>) =>
    announcerSpokenText({
      tool: 'codex',
      kind: 'command',
      summary: 'codex ran a command',
      detail: { command: text, ...detail },
    });

  it('says whether tests passed or failed when the tool reports an exit code', () => {
    expect(ran('/bin/zsh -lc cargo test', { exit_code: 0 })).toBe('I ran the Rust tests, and they passed.');
    expect(ran('/bin/zsh -lc cargo test', { exit_code: 101 })).toBe('I ran the Rust tests, and they failed.');
    expect(ran('npm run build', { exitCode: 2 })).toBe('I built the project, and it failed.');
    expect(ran('npm run build', { exitCode: 0 })).toBe('I built the project.');
    expect(ran('git push', { status: 'failed' })).toBe('I pushed changes, and it failed.');
    expect(ran('npx vitest run', {})).toBe('I ran the unit tests.');
    expect(commandOutcome({ exit_code: 'x' })).toBeUndefined();
    expect(commandOutcome([1])).toBeUndefined();
  });

  it('classifies every kind of event', () => {
    const step = (kind: string, summary: string, detail: unknown = null) =>
      announcerStep({ tool: 'claude', kind, summary, detail });
    expect(step('edit', 'claude edited a/b.ts', { path: 'a/b.ts' })).toEqual({
      phrase: 'edited b.ts',
      category: 'edit',
      files: ['b.ts'],
    });
    expect(step('edit', 'claude edited a file')).toEqual({ phrase: 'edited a file', category: 'edit', files: [] });
    expect(step('tool_call', 'claude called Grep', { name: 'Grep' }).category).toBe('search');
    expect(step('tool_call', 'Fetched the notes', { name: 'WebFetch' })).toEqual({ phrase: '', category: 'other' });
    expect(step('turn_end', 'claude finished a turn').category).toBe('milestone');
    expect(step('permission', 'claude is asking permission').category).toBe('milestone');
    expect(step('note', 'synthetic note')).toEqual({ phrase: 'left a note', category: 'other' });
    expect(step('summary', 'dropped 4 events').category).toBe('other');
    expect(toolCallIntent('mcp__hotsheet__hotsheet_claim').category).toBe('ticket');
    expect(toolCallIntent('mcp__hotsheet__hotsheet_get').category).toBe('ticket-work');
    expect(toolCallIntent('Task').category).toBe('milestone');
    expect(toolCallIntent('Read').category).toBe('read');
  });
});

describe('the agent speaks for itself (HS2-G3HJZH)', () => {
  it('turns prose that opens with the tool name into the first person', () => {
    expect(firstPerson('codex is blocked: waiting for review', 'codex')).toBe('I am blocked: waiting for review');
    expect(firstPerson('claude is asking permission', 'claude')).toBe('I am asking permission');
    expect(firstPerson('Claude has a question', 'claude')).toBe('I have a question');
    expect(firstPerson('codex was interrupted', 'codex')).toBe('I was interrupted');
    expect(firstPerson('codex decided: keep it short', 'codex')).toBe('I decided: keep it short');
    expect(firstPerson('claude sub-agent: reading the docs', 'claude')).toBe('My sub-agent: reading the docs');
    expect(firstPerson('claude background task completed: build', 'claude')).toBe(
      'My background task completed: build',
    );
    expect(firstPerson("claude's plan is ready", 'claude')).toBe('My plan is ready');
  });

  it('leaves other prose alone', () => {
    expect(firstPerson('Updated the synthetic sample file', 'codex')).toBe('Updated the synthetic sample file');
    expect(firstPerson('codexify the docs', 'codex')).toBe('codexify the docs');
    expect(firstPerson('codex: done', 'codex')).toBe('codex: done');
    expect(firstPerson('codex', 'codex')).toBe('codex');
    expect(firstPerson('claude is here', ' ')).toBe('claude is here');
  });

  it('says commands, tool calls, edits, and prose events as "I"', () => {
    const say = (kind: string, summary: string, detail: unknown = null) =>
      announcerSpokenText({ tool: 'codex', kind, summary, detail });
    expect(say('command', 'codex ran `git push origin main`', { command: 'git push origin main' })).toBe(
      'I pushed changes to main.',
    );
    expect(say('tool_call', 'codex called Grep', { name: 'Grep' })).toBe('I searched the code.');
    expect(say('edit', 'codex edited src/a.ts', { path: 'src/a.ts' })).toBe('I edited a.ts.');
    expect(say('blocked', 'codex is blocked: `npm ci` needs the network')).toBe(
      'I am blocked: a command needs the network',
    );
  });
});
