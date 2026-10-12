import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { CommandRunDialog } from './command-run-dialog';

const command = { id: 'test', title: 'Test project', program: 'npm', args: ['test'] };
const run = {
  id: 'run-1',
  command_id: 'test',
  state: 'running' as const,
  output: [{ seq: 1, stream: 'stdout', text: 'ok' }],
};

describe('CommandRunDialog', () => {
  it('keeps only the native shell and output rules and composes Kerf layout (HS2-X7T5ZR)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'command-run-dialog.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).not.toContain('[data-component');
    expect(css).not.toMatch(/\bsvg\b|\bbutton\b|__title|__summary|__actions/);
    expect([...css.matchAll(/^\.[\w-]+ \{/gm)].map((match) => match[0])).toEqual([
      '.command-run-dialog {',
      '.command-run-dialog__output {',
    ]);
    const output = String(CommandRunDialog({ command, run })),
      stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    for (const markup of [output, stop]) {
      expect(markup).toMatch(/<dialog class="command-run-dialog"/);
      expect(markup).toContain('kui-row');
      expect(markup).toMatch(/<h2 class="kui-text/);
      expect(markup).toMatch(/<svg[^>]*data-size="13\.6"[^>]*--_kui-lucide-size:0\.85rem/);
      expect(markup).not.toContain('<button');
    }
    expect(output).toContain('class="command-run-dialog__output"');
    expect(stop).toMatch(/<wa-button variant="danger"[^>]*data-action="confirm-stop-command"/);
  });

  it('renders command output and a stop confirmation as distinct surfaces', () => {
    expect(String(CommandRunDialog({ command, run }))).toContain('aria-label="Command output"');
    const stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    expect(stop).toContain('Stop Test project?');
    expect(stop).toContain('data-action="confirm-stop-command"');
    expect(stop).toContain('data-run-id="run-1"');
  });
});
