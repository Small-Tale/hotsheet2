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
  it('uses Kerf semantic spacing around dialog regions, output, actions, and icon labels', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'command-run-dialog.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(
      /command-run-dialog__summary \{[^}]*margin-bottom: var\(--kui-space-m\)[^}]*gap: var\(--kui-space-xs\)/,
    );
    expect(css).toMatchSource(/\.command-run-dialog__output \{[^}]*padding: var\(--kui-space-xs\)/);
    expect(css).toMatchSource(/\.command-run-dialog \{[^}]*padding: var\(--kui-space-l\)/);
  });

  it('styles only its own classes and sizes icons through the LucideIcon prop (HS2-29Q3XG)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'command-run-dialog.css'), 'utf8');
    expect(css).not.toContain('[data-component');
    expect(css).not.toMatch(/\bsvg\b/);
    const output = String(CommandRunDialog({ command, run })),
      stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    for (const markup of [output, stop]) {
      expect(markup).toMatch(/<dialog class="command-run-dialog"/);
      expect(markup).toContain('class="command-run-dialog__title"');
      expect(markup).toMatch(/<svg[^>]*data-size="13\.6"[^>]*--_kui-lucide-size:0\.85rem/);
    }
    expect(output).toContain('class="command-run-dialog__output"');
  });

  it('renders command output and a stop confirmation as distinct surfaces', () => {
    expect(String(CommandRunDialog({ command, run }))).toContain('aria-label="Command output"');
    const stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    expect(stop).toContain('Stop Test project?');
    expect(stop).toContain('data-action="confirm-stop-command"');
    expect(stop).toContain('data-run-id="run-1"');
  });
});
