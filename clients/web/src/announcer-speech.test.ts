import { describe, expect, it } from 'vitest';

import { announcerItem } from './announcer-reel';
import { announcerSpokenText, commandPhrase, speakableProse } from './announcer-speech';

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
    expect(spoken).toMatch(/^claude [a-z ]+\.$/i);
    expect(spoken).not.toContain('`');
    expect(spoken).not.toContain(text);
    // No path, flag, quoted argument, or other non-word fragment of the command is spoken.
    for (const fragment of text.split(/\s+/).filter((word) => /[^A-Za-z]/.test(word) && word.length > 1))
      expect(spoken).not.toContain(fragment);
  });

  it('falls back to the summary code span when the detail has no command', () => {
    expect(
      announcerSpokenText({
        tool: 'codex',
        kind: 'command',
        summary: 'codex ran `git push origin main`',
        detail: null,
      }),
    ).toBe('codex pushed changes.');
    expect(announcerSpokenText({ tool: 'codex', kind: 'command', summary: 'codex ran a command', detail: {} })).toBe(
      'codex ran a command.',
    );
  });

  it('names what common commands do', () => {
    expect(commandPhrase('git status --short')).toBe('checked the repository');
    expect(commandPhrase('git commit -m x')).toBe('committed changes');
    expect(commandPhrase('git pull --rebase')).toBe('updated from the remote');
    expect(commandPhrase('git rebase origin/main')).toBe('integrated changes');
    expect(commandPhrase('git stash list')).toBe('used Git');
    expect(commandPhrase('scripts/check.sh --plan')).toBe('ran tests');
    expect(commandPhrase('npx vitest run')).toBe('ran tests');
    expect(commandPhrase('npm run lint')).toBe('checked formatting and lint');
    expect(commandPhrase('cargo build --release')).toBe('built the project');
    expect(commandPhrase('npm install')).toBe('installed dependencies');
    expect(commandPhrase('rg -n sample src')).toBe('searched the code');
    expect(commandPhrase('cat > notes.md')).toBe('wrote a file');
    expect(commandPhrase('sed -n 1,20p a.ts')).toBe('read files');
    expect(commandPhrase('mkdir -p out')).toBe('rearranged files');
    expect(commandPhrase('curl -s https://example.test')).toBe('made a network request');
    expect(commandPhrase('hotsheet-cli ls --up-next')).toBe('updated Hot Sheet tickets');
    expect(commandPhrase('sudo env A=1 make')).toBe('ran a command');
    expect(commandPhrase('')).toBe('ran a command');
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
    ).toBe('claude edited sample.ts.');
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
      'codex reported turn end.',
    );
    expect(announcerSpokenText({ tool: ' ', kind: 'command', summary: '', detail: null })).toBe(
      'The agent ran a command.',
    );
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
    expect(item.spoken).toBe('claude wrote a file.');
  });
});
