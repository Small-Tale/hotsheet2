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
    expect(markup).toContain('Project One');
    expect(markup).toContain('name="terminal-rail-view"');
    expect(markup).toContain('Queue');
    expect(markup).toContain('aria-label="Hide ticket rail"');
    expect(markup).toContain('class="kui-sunken-panel terminal-ticket-rail__content"');
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
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8');
    expect(css).toMatchSource(/\.view-mode-switcher \{[^}]*grid-column:1 \/ -1/);
    expect(css).toMatchSource(/workspace-header__utility-group \{[^}]*grid-column:2/);
    expect(css).toMatchSource(
      /workspace-header__search-group \{[^}]*grid-column:3[^}]*grid-row:2[^}]*transition:width \.25s ease/,
    );
    expect(css).toMatchSource(
      /workspace-header__search-group\[data-expanded="true"\] \{[^}]*width:100%[^}]*grid-column:1 \/ -1[^}]*grid-row:3[^}]*animation:terminal-ticket-rail-search-enter \.25s ease/,
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
    expect(css).toMatchSource(/__project \{[^}]*padding-inline:var\(--kui-space-xs\)/);
    expect(css).toMatchSource(
      /__controls \{ padding:var\(--kui-space-2xs\) var\(--kui-space-xs\) var\(--kui-space-xs\)/,
    );
    expect(css).toMatchSource(/__heading \{[^}]*padding:var\(--kui-space-xs\)/);
    expect(css).not.toContain('--kui-sunken-panel-radius');
    expect(css).toMatch(/translateY\(calc\(-100% - var\(--kui-space-xs\)\)\)/);
    expect(css).toMatchSource(/__project \{ min-height:remify\(52px\)/);
    expect(css).toMatchSource(/__heading \{ min-height:remify\(60px\)/);
  });
  it('separates the heading from the ticket scroller and preserves the shared compact launcher', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8'),
      markup = String(
        TerminalTicketRail({ ...props, active: 'root', action: QuickTicketLauncher({ label: 'Ticket…' }) }),
      ),
      heading = markup.match(/<header class="kui-toolbar terminal-ticket-rail__heading"[\s\S]*?<\/header>/)![0];
    expect(css).toMatchSource(/__heading \{[^}]*border-bottom:1px solid var\(--wa-color-surface-border\)/);
    expect(heading).toContain('class="quick-ticket-composer__launcher"');
    expect(heading).toContain('Ticket…');
    expect(heading).not.toContain('kui-toolbar-control-group');
  });
  it('keeps the ticket collection intrinsic so the rail surface owns vertical scrolling', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8');
    expect(css).toMatchSource(/__content \{[^}]*overflow:auto[^}]*flex:1/);
    expect(css).toMatchSource(/__content > \.ticket-list \{[^}]*flex:none/);
  });
});
