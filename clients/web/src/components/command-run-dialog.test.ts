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
  it('keeps only the output rule and renders a Kerf DialogSurface dialog (HS2-X7T5ZR, HS2-Z7C27E, HS2-BH7NZ4)', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'command-run-dialog.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).not.toContain('[data-component');
    expect(css).not.toMatch(/\bsvg\b|\bbutton\b|__title|__summary|__actions/);
    expect([...css.matchAll(/^\.[\w-]+ \{/gm)].map((match) => match[0])).toEqual(['.command-run-dialog__output {']);
    const output = String(CommandRunDialog({ command, run })),
      stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    for (const markup of [output, stop]) {
      // Kerf's native dialog, opened by the controlled-open pass rather than an `open` attribute.
      expect(markup).not.toContain('<wa-dialog');
      expect(markup).toMatch(/<dialog[^>]*data-controlled-open="true"[^>]*class="kui-dialog-surface"/);
      expect(markup).toContain('data-component="dialog-surface"');
      // Kerf's header close and the confirmation actions are native buttons.
      expect(markup.match(/<button/g)).toHaveLength(markup === output ? 1 : 3);
      expect(markup).toMatch(/<button[^>]*class="kui-dialog-surface__close"/);
    }
    // The output dialog relies on Kerf's header close; only the stop confirmation has a footer.
    expect(output).not.toContain('kui-dialog-surface__footer');
    expect(stop).toMatch(/<svg[^>]*data-size="13\.6"[^>]*--_kui-lucide-size:0\.85rem/);
    expect(output).toContain('class="command-run-dialog__output"');
    expect(stop).toMatch(/<footer class="kui-dialog-surface__footer"><div class="kui-row"/);
    expect(stop).toMatch(/<button(?=[^>]*data-action="confirm-stop-command")(?=[^>]*data-tone="danger")/);
  });

  it('renders command output and a stop confirmation as distinct surfaces', () => {
    expect(String(CommandRunDialog({ command, run }))).toContain('aria-label="Command output"');
    const stop = String(CommandRunDialog({ command, run, confirmStop: true }));
    expect(stop).toContain('aria-label="Stop Test project?"');
    expect(stop).toContain('data-action="confirm-stop-command"');
    expect(stop).toContain('data-run-id="run-1"');
  });

  it('lists shell and AI launches with their ticket, terminal, and time instead of output (HS2-PWNPM2)', () => {
    const launch = (id: string, value: object) => ({
      id,
      command_id: 'everything',
      state: 'completed' as const,
      output: [],
      started_at: '2026-10-28T10:00:00Z',
      launch: value as never,
    });
    const ai = { id: 'everything', title: 'Everything', kind: 'ai' as const, prompt: 'Do it' };
    const markup = String(
      CommandRunDialog({
        command: ai,
        launches: [
          launch('l2', { kind: 'ai', ticket_slug: 'HS2-BBBBBB', tool: 'claude' }),
          launch('l1', { kind: 'ai', ticket_slug: 'HS2-AAAAAA' }),
        ],
        launchTickets: { 'HS2-BBBBBB': { title: 'Everything', status: 'started' } },
      }),
    );
    expect(markup).toContain('aria-label="Recent launches"');
    expect(markup).not.toContain('Never run');
    expect(markup).not.toContain('aria-label="Command output"');
    // Newest first; each slug opens its ticket through the shared linked-ticket action.
    expect(markup.indexOf('HS2-BBBBBB')).toBeLessThan(markup.indexOf('HS2-AAAAAA'));
    expect(markup).toMatch(
      /<a href="#ticket-HS2-BBBBBB"[^>]*data-action="open-linked-ticket"[^>]*data-ticket-slug="HS2-BBBBBB"/,
    );
    // A loaded ticket shows its title and status; an unloaded one shows only its slug.
    expect(markup).toContain('data-component="text"');
    expect(markup).toContain('Started');
    expect(markup.match(/data-command-launch=/g)).toHaveLength(2);
    expect(markup).toContain('<time datetime="2026-10-28T10:00:00Z">');
    const shell = String(
      CommandRunDialog({
        command: { id: 'build', title: 'Build', kind: 'shell', command: 'make' },
        launches: [launch('s1', { kind: 'shell', terminal_label: 'Build' })],
      }),
    );
    expect(shell).toContain('Opened in terminal Build');
    // Shell and AI commands with no launches say so without an empty output box.
    const never = String(CommandRunDialog({ command: ai }));
    expect(never).toContain('Never run');
    expect(never).not.toContain('No output recorded.');
  });
});
