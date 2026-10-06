import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { TerminalTicketRail, terminalTicketRailPanel } from './terminal-ticket-rail';
import { ticketInspectorPanel } from './ticket-inspector';

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
  };
  const detail = {
    key: 'ticket:HS2-TEST',
    parts: ticketInspectorPanel({
      slug: 'HS2-TEST',
      title: 'Pushed ticket',
      status: 'started',
      priority: 'default',
      category: 'task',
      tags: [],
      details: '',
    }),
  };
  it('starts on a compact project ticket surface whose project menu can grow to fit longer names', () => {
    const markup = String(TerminalTicketRail({ ...props, collapseControl: true }));
    expect(markup).toContain('data-component="nav-stack"');
    expect(markup).toContain('name="terminal-rail-project"');
    expect(markup).toMatch(/data-component="toolbar-control-group"[^>]*><wa-select[^>]*name="terminal-rail-project"/);
    expect(markup).toContain('Project One');
    expect(markup).toContain('name="terminal-rail-view"');
    expect(markup).toContain('Queue');
    expect(markup).toContain('aria-label="Hide ticket rail"');
    expect(markup).toContain('data-action="toggle-ticket-inspector"');
    expect(markup).toContain('data-component="terminal-ticket-rail-list"');
    expect(markup.match(/<wa-select[^>]*name="terminal-rail-project"[^>]*>/)?.[0]).not.toContain(
      'kui-select--fit-menu',
    );
    const viewSelect = markup.match(/<wa-select[^>]*name="terminal-rail-view"[^>]*>/)?.[0];
    expect(viewSelect).not.toContain('kui-select--fit-menu');
    // The view title is Kerf's title Select with an inset ring, not an app className hook (HS2-DAMHD1).
    expect(viewSelect).toContain('data-presentation="title"');
    expect(viewSelect).toContain('data-focus-ring-inset="true"');
    expect(viewSelect).not.toContain('terminal-ticket-rail__view-select');
  });
  it('is a Workbench navigation panel whose root view lists tickets under its pinned controls (HS2-FY06N4)', () => {
    const parts = terminalTicketRailPanel(props);
    expect(parts.toggle).toEqual({ action: 'toggle-ticket-inspector', name: 'ticket rail' });
    expect(parts.navStack.backLabel).toBe('Back to ticket list');
    expect(parts.navStack.toolbarConfig?.centerAlign).toBeUndefined();
    expect(parts.navStack.views.map((view) => view.key)).toEqual(['root']);
    const [root] = parts.navStack.views;
    // The project selector is the root view's toolbar group, ahead of the panel's standard toggle.
    expect(String(root.toolbar?.leading)).toContain('name="terminal-rail-project"');
    expect(String(root.header)).toContain('class="kui-toolbar terminal-ticket-rail__controls"');
    expect(String(root.header)).toContain('name="terminal-rail-view"');
    expect(root.pane?.appearance).toBe('sunken');
    expect(String(root.content)).toContain('Tickets');
    expect(String(root.content)).not.toContain('name="terminal-rail-project"');
  });
  it('pushes the ticket detail as one toolbar row with its pinned header and scrolling body (HS2-FY06N4)', () => {
    const parts = terminalTicketRailPanel({ ...props, detail });
    expect(parts.navStack.views.map((view) => view.key)).toEqual(['root', 'ticket:HS2-TEST']);
    const pushed = parts.navStack.views[1];
    // Kerf renders the back control first and the panel toggle last; the view adds the ticket
    // number (centered) and its actions — no second toolbar and no hand-made back button.
    expect(String(pushed.toolbar?.leading)).toContain('data-action="copy-ticket-slug"');
    expect(String(pushed.toolbar?.trailing)).toContain('data-action="open-ticket-reader"');
    expect(String(pushed.header)).toContain('data-component="ticket-inspector-header"');
    expect(String(pushed.header)).toContain('Pushed ticket');
    expect(String(pushed.content)).toContain('data-component="ticket-inspector-body"');
    const markup = String(TerminalTicketRail({ ...props, detail, collapseControl: true }));
    expect(markup).toContain('data-nav-back');
    expect(markup).toContain('aria-label="Back to ticket list"');
    expect(markup).not.toContain('terminal-ticket-rail__back');
    expect(markup.match(/data-component="toolbar" /g)?.length).toBeGreaterThan(0);
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
    // The composed groups take their rail presentation through their own props: the view switcher's
    // full-row Kerf `sizing="fill"` and the search field's `layout="row"` placement and entrance
    // (HS2-8FS5BJ, HS2-DAMHD1). This stylesheet styles no other component.
    expect(css).not.toContain('view-mode-switcher');
    expect(css).not.toContain('ticket-search-field');
    expect(css).not.toContain('quick-ticket-composer');
    expect(css).not.toContain('notification-center');
    expect(css).not.toContain('.kui-');
    const headerCss = readFileSync(new URL('./workspace-header.css', import.meta.url), 'utf8'),
      searchCss = readFileSync(new URL('./ticket-search-field.css', import.meta.url), 'utf8');
    expect(headerCss).not.toContain('view-mode-switcher--rail');
    expect(searchCss).not.toContain('ticket-search-field--row');
    expect(css).not.toMatch(/ticket-inspector__header > \.kui-toolbar \{[^}]*padding-left/);
    // Kerf beta.62 balances the toolbar tracks itself (`centerAlign="balanced"`).
    expect(css).not.toContain('ticket-inspector__header > .kui-toolbar');
    expect(css).not.toContain('.kui-toolbar-text');
    // Kerf's NavStack renders the back control; the app styles none of its own (HS2-FY06N4).
    expect(css).not.toContain('terminal-ticket-rail__back');
  });
  it('uses the canonical compact-rail spacing while retaining control and transition geometry', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8');
    expect(css).not.toContain('--wa-space-');
    // The project Select sits in Kerf's panel toolbar; the app only sets public Select tokens on it.
    expect(css).toMatchSource(/__project \{ --kui-select-selected-color:/);
    expect(css).not.toContain('--kui-toolbar-trailing-justify-self');
    expect(css).not.toMatch(/__project \{[^}]*\b(padding|min-height|height):/);
    expect(css).not.toContain('.kui-select__custom-selected');
    expect(css).not.toMatch(/\.kui-select \{/);
    expect(css).not.toMatch(/__project \.kui-toolbar__/);
    // The controls Toolbar keeps Kerf's own inset; the app adds no rule on that toolbar root (HS2-K9KWJJ).
    expect(css).not.toMatch(/__controls \{/);
    // The heading wrapper only draws the rule; the Toolbar inside keeps Kerf's inset and height.
    expect(css).toMatchSource(/__heading \{ display:grid;border-bottom:/);
    expect(css).not.toContain('--kui-sunken-panel-radius');
    // The rail search's row entrance is Kerf's stacked fill-search motion (KF-XFPJSY, HS2-DAMHD1).
    expect(readFileSync(new URL('./ticket-search-field.css', import.meta.url), 'utf8')).not.toContain('translateY');
    // The view-title Select's 36px geometry and inset focus ring are Kerf's `presentation="title"` and
    // `focusRingInset` (KF-PZ23ZP, HS2-DAMHD1); this module styles no Select part.
    expect(css).not.toContain('::part(');
    expect(css).not.toContain('view-select');
    expect(css).not.toMatch(/__heading \{[^}]*min-height/);
  });
  it('separates the heading from the ticket scroller and preserves the shared compact launcher', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8'),
      markup = String(
        TerminalTicketRail({ ...props, action: { kind: 'new-ticket', label: 'Ticket…', size: 'compact' } }),
      ),
      heading = markup.match(
        /<div class="terminal-ticket-rail__heading"><header class="kui-toolbar"[\s\S]*?<\/header>/,
      )![0];
    expect(css).toMatchSource(/__heading \{[^}]*border-bottom:1px solid var\(--wa-color-surface-border\)/);
    // The rail asks the launcher for its compact size instead of restyling it (HS2-8FS5BJ).
    expect(heading).toContain('class="quick-ticket-composer__launcher" data-size="compact"');
    expect(css).not.toContain('quick-ticket-composer');
    expect(heading).toContain('Ticket…');
    expect(heading).not.toContain('kui-toolbar-control-group');
  });
  it('lets the NavStack view own vertical scrolling instead of an app scroller (HS2-FY06N4)', () => {
    const css = readFileSync(new URL('./terminal-ticket-rail.css', import.meta.url), 'utf8');
    expect(css).not.toContain('__content');
    expect(css).not.toContain('content-transition');
    expect(css).not.toContain('__back');
  });
});
