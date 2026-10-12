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
  it('keeps only the output rule and composes wa-dialog inside Kerf DialogSurface (HS2-X7T5ZR, HS2-Z7C27E)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'command-run-dialog.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).not.toContain('[data-component');
    expect(css).not.toMatch(/\bsvg\b|\bbutton\b|__title|__summary|__actions/);
    expect([...css.matchAll(/^\.[\w-]+ \{/gm)].map((match) => match[0])).toEqual([
      '.command-run-dialog__output {',
    ]);
    const output = String(CommandRunDialog({ command, run })),
      stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    for (const markup of [output, stop]) {
      expect(markup).not.toContain('<dialog');
      expect(markup).toMatch(/<wa-dialog[^>]* open[ >]/);
      expect(markup).toContain('data-component="dialog-surface"');
      expect(markup).not.toContain('<button');
    }
    // The output dialog relies on Web Awesome's header close; only the stop confirmation has a footer.
    expect(output).not.toContain('slot="footer"');
    expect(stop).toMatch(/<svg[^>]*data-size="13\.6"[^>]*--_kui-lucide-size:0\.85rem/);
    expect(output).toContain('class="command-run-dialog__output"');
    expect(stop).toMatch(/kui-row[^"]*"[^>]*slot="footer"|slot="footer"[^>]*kui-row/);
    expect(stop).toMatch(/<wa-button variant="danger"[^>]*data-action="confirm-stop-command"/);
  });

  it('renders command output and a stop confirmation as distinct surfaces', () => {
    expect(String(CommandRunDialog({ command, run }))).toContain('aria-label="Command output"');
    const stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    expect(stop).toContain('label="Stop Test project?"');
    expect(stop).toContain('data-action="confirm-stop-command"');
    expect(stop).toContain('data-run-id="run-1"');
  });
});
