import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { QuickTicketLauncher } from './quick-ticket-composer';
import { TerminalTicketRail } from './terminal-ticket-rail';

describe('TerminalTicketRail', () => {
  const props = {
    projects: [
      { id: 'one', name: 'Project One' },
      { id: 'two', name: 'Project Two' },
    ],
    selectedProjectId: 'one',
    views: [
      { id: 'all', label: 'Queue' },
      { id: 'backlog', label: 'Backlog' },
    ],
    selectedViewId: 'all',
    controls: 'Controls' as never,
    content: 'Tickets' as never,
    inspector: 'Inspector' as never,
  };
  it('starts on a compact project ticket surface whose project menu can grow to fit longer names', () => {
    const markup = String(TerminalTicketRail({ ...props, active: 'root' }));
    expect(markup).toContain('name="terminal-rail-project"');
    expect(markup).toMatch(/data-component="toolbar-control-group"[^>]*><wa-select[^>]*name="terminal-rail-project"/);
    expect(markup).toContain('Project One');
    expect(markup).toContain('name="terminal-rail-view"');
    expect(markup).toContain('Queue');
    expect(markup).toContain('aria-label="Hide ticket rail"');
    expect(markup).toContain('<div class="terminal-ticket-rail__content"><div class="kui-sunken-panel"');
    expect(markup).toContain('data-component="sunken-panel"');
    expect(markup).toContain('data-shape="square"');
    expect(markup).toContain('data-active-side="a"');
    expect(markup.match(/<wa-select[^>]*name="terminal-rail-project"[^>]*>/)?.[0]).not.toContain(
      'kui-select--fit-menu',
    );
    expect(markup.match(/<wa-select[^>]*name="terminal-rail-view"[^>]*>/)?.[0]).not.toContain('kui-select--fit-menu');
  });
  it('uses the shared backward pop when returning from a ticket', () => {
    const markup = String(TerminalTicketRail({ ...props, active: 'root', direction: 'backward' }));
    expect(markup).toContain('data-active-side="a"');
    expect(markup).toContain('data-transition-style="push"');
    expect(markup).toContain('data-transition-direction="backward"');
  });
  it('keeps compact search last, animates active search onto a full row, and centers the ticket header independently', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8'),
      source = readFileSync(new URL('./terminal-ticket-rail.tsx', import.meta.url), 'utf8');
    // The controls are the trailing zone of a cataloged Toolbar that stacks and wraps at group
    // granularity; no app-owned grid places them (HS2-K9KWJJ).
    expect(source).toMatch(
      /<Toolbar\s+className="terminal-ticket-rail__controls"[^>]*responsive="stack"[^>]*responsiveAt="narrow"[^>]*trailing=\{controls\}/,
    );
    expect(css).not.toMatch(/__controls \{/);
    expect(css).not.toMatch(/__controls[^{]*\{[^}]*grid-(?:column|row)/);
    expect(css).toMatchSource(/__controls \.view-mode-switcher \{[^}]*width:100%/);
    expect(css).toMatchSource(
      /__controls \.ticket-search-field \{[^}]*width:remify\([\d.]+px\)[^}]*margin-inline-start:auto[^}]*transition:width \.25s ease/,
    );
    expect(css).toMatchSource(
      /__controls \.ticket-search-field\[data-content="search"\]\[data-expanded="true"\] \{[^}]*width:100%[^}]*animation:terminal-ticket-rail-search-enter \.25s ease/,
    );
    expect(css).not.toMatch(/ticket-inspector__header > \.kui-toolbar \{[^}]*padding-left/);
    expect(css).toMatchSource(/ticket-inspector__header > \.kui-toolbar \{[^}]*grid-template-columns:1fr auto 1fr/);
    expect(css).toMatchSource(
      /terminal-ticket-rail__back \{[^}]*width:remify\(36px\)[^}]*color:var\(--wa-color-brand-on-quiet\)/,
    );
  });
  it('uses the canonical compact-rail spacing while retaining control and transition geometry', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8');
    expect(css).not.toContain('--wa-space-');
    // The project Toolbar keeps Kerf's own inset and height; the app only sets public tokens on it.
    expect(css).toMatchSource(/__project \{ --kui-toolbar-trailing-justify-self:end;/);
    expect(css).not.toMatch(/__project \{[^}]*\b(padding|min-height|height):/);
    expect(css).not.toContain('.kui-select__custom-selected');
    expect(css).not.toMatch(/\.kui-select \{/);
    expect(css).not.toMatch(/__project \.kui-toolbar__/);
    // The controls Toolbar keeps Kerf's own inset; the app adds no rule on that toolbar root (HS2-K9KWJJ).
    expect(css).not.toMatch(/__controls \{/);
    // The heading wrapper only draws the rule; the Toolbar inside keeps Kerf's inset and height.
    expect(css).toMatchSource(/__heading \{ display:grid;border-bottom:/);
    expect(css).not.toContain('--kui-sunken-panel-radius');
    expect(css).toMatch(/translateY\(calc\(-100% - var\(--kui-space-xs\)\)\)/);
    expect(css).not.toMatch(/__heading \{[^}]*min-height/);
  });
  it('separates the heading from the ticket scroller and preserves the shared compact launcher', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8'),
      markup = String(
        TerminalTicketRail({ ...props, active: 'root', action: QuickTicketLauncher({ label: 'Ticket…' }) }),
      ),
      heading = markup.match(
        /<div class="terminal-ticket-rail__heading"><header class="kui-toolbar"[\s\S]*?<\/header>/,
      )![0];
    expect(css).toMatchSource(/__heading \{[^}]*border-bottom:1px solid var\(--wa-color-surface-border\)/);
    expect(heading).toContain('class="quick-ticket-composer__launcher"');
    expect(heading).toContain('Ticket…');
    expect(heading).not.toContain('kui-toolbar-control-group');
  });
  it('keeps the ticket collection intrinsic so the rail surface owns vertical scrolling', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8');
    expect(css).toMatchSource(/__content \{[^}]*overflow:auto[^}]*flex:1/);
    expect(css).toMatchSource(/__content > \* > \.ticket-list \{[^}]*flex:none/);
    expect(css).toMatchSource(/__content \{[^}]*grid-template-rows:minmax\(100%, max-content\)/);
  });
});
