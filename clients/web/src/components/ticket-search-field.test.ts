import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { createTicketSearchModel, replaceTicketSearch } from '../ticket-search-model';
import {
  TICKET_SEARCH_ACTIONS,
  TicketSearchField,
  TicketSearchFormField,
  TicketSearchSurfaces,
} from './ticket-search-field';

const tags = ['client', 'Docs', 'needs design', 'server'];

/** A Kerf-managed model: `committed` goes through the app parser, `typing` stays trailing editor text. */
function model(committed = '', typing = '') {
  const result = createTicketSearchModel({ tags: () => tags });
  if (committed) replaceTicketSearch(result, committed);
  if (typing) result.edit({ query: `${result.state.value.query}${typing}`, tokens: result.state.value.tokens });
  return result;
}

describe('TicketSearchField (HS2-N5G6JS, HS2-5JXBQY)', () => {
  it("composes Kerf's grouped token field with generic, id-resolved actions over the managed model", () => {
    const markup = String(
      TicketSearchField({ id: 'demo-search', label: 'Search tickets', model: model('is:open', 'parser') }),
    );
    expect(markup).toContain('class="kui-toolbar-control-group ticket-search-field"');
    expect(markup).toMatch(/ticket-search-field"[^>]*data-expanded="true"/);
    expect(markup).toContain('data-content="search"');
    // The token colors sit on the app-owned hook Kerf renders on its field root (HS2-8FS5BJ).
    expect(markup).toContain('class="kui-token-search ticket-search-field__query"');
    expect(markup).toContain('data-component="token-search-field" data-token-search-id="demo-search"');
    expect(markup).toContain('role="searchbox" aria-label="Search tickets"');
    expect(markup).toContain('data-token-value="is:open"');
    expect(markup).toContain('>parser</span>');
    expect(markup).toContain(`data-action="${TICKET_SEARCH_ACTIONS.editToken.value}"`);
    expect(markup).toContain(`data-action="${TICKET_SEARCH_ACTIONS.removeToken.value}"`);
    expect(markup).toContain(`data-action="${TICKET_SEARCH_ACTIONS.clear.value}"`);
    expect(markup).toContain('aria-label="Clear search"');
    expect(markup).toMatch(
      new RegExp(
        `class="ticket-search-field__help-button" data-action="${TICKET_SEARCH_ACTIONS.toggleHelp.value}" aria-label="Search syntax help" aria-expanded="false"`,
      ),
    );
    expect(markup).toContain('data-lucide="circle-help"');
    expect(markup).toContain('data-collapsible="false" data-expanded="true"');
    expect(markup).not.toContain('kui-token-search__suggestions');
    expect(markup).not.toContain('aria-label="Date and time helper"');
    expect(markup).not.toContain('aria-label="Search syntax"');
    expect(markup).not.toContain('workspace-search');
  });

  it("renders Kerf's in-flow tag suggestions from the project tags and the trailing tag: text", () => {
    const suggest = (committed: string, typing: string) =>
      [
        ...String(TicketSearchField({ id: 'f', label: 'Search', model: model(committed, typing) })).matchAll(
          /data-token-search-suggestion="([^"]+)"/g,
        ),
      ].map((match) => match[1]);
    // Case-insensitive prefix, committed chips excluded, quoted values for spaces.
    // Attribute values arrive HTML-escaped from the string renderer.
    expect(suggest('tag:client', 'parser tag:')).toEqual(['tag:Docs', 'tag:&quot;needs design&quot;', 'tag:server']);
    expect(suggest('tag:client', 'tag:D')).toEqual(['tag:Docs']);
    expect(suggest('tag:client', 'tag:"nee')).toEqual(['tag:&quot;needs design&quot;']);
    expect(suggest('tag:client', 'tag:cl')).toEqual([]);
    expect(suggest('', 'tag:cl')).toEqual(['tag:client']);
    expect(suggest('tag:client', 'tag:client parser')).toEqual([]);
    expect(suggest('', '')).toEqual([]);
    const markup = String(TicketSearchField({ id: 'f', label: 'Search', model: model('', 'tag:n') }));
    expect(markup).toContain('class="kui-token-search__suggestions" data-token-search-keep-open');
    expect(markup).toContain(
      '<button type="button" class="kui-token-search__suggestion" data-token-search-suggestion="tag:&quot;needs design&quot;">tag:&quot;needs design&quot;</button>',
    );
  });

  it('offers the date helper for a trailing lifecycle filter and the syntax help on request', () => {
    const date = String(TicketSearchField({ id: 'f', label: 'Search', model: model('', 'updated-after:') }));
    expect(date).toContain('role="group" aria-label="Date and time helper"');
    expect(date).toContain('name="ticket-search-date" type="date"');
    expect(date).toContain('name="ticket-search-time" type="time"');
    expect(date).toContain(`data-action="${TICKET_SEARCH_ACTIONS.applyDate.value}" data-date-prefix="updated-after"`);
    expect(date).not.toContain('aria-label="Search syntax"');
    const help = String(TicketSearchField({ id: 'f', label: 'Search', model: model(), helpOpen: true }));
    expect(help).toContain('aria-expanded="true"');
    expect(help).toContain('class="ticket-search-field__help" role="dialog" aria-label="Search syntax"');
    expect(help).toContain('<dt>Tags</dt>');
    expect(help).toContain('<dt>Content</dt>');
    expect(help).toContain('<dt>Workflow</dt>');
    expect(help).toContain('<dt>Dates</dt>');
    expect(help).toContain('<code>is:duplicate</code>');
    expect(help).toContain('<strong>Combine filters</strong>');
    expect(help).toContain('updated-after:2026-09-01T11:05');
    expect(help).toContain('local, relative, and ISO 8601 dates work');
    const noHelp = String(TicketSearchField({ id: 'f', label: 'Search', model: model(), helpOpen: true, help: false }));
    expect(noHelp).not.toContain('Search syntax');
  });

  it('collapses every helper surface with a closed collapsible field and reopens them with it', () => {
    // Kerf keeps a populated collapsible field open, so the closed state is the empty one.
    const props = { id: 'f', label: 'Search', helpOpen: true, collapsible: true } as const;
    const closed = String(TicketSearchField({ ...props, model: model(), expanded: false }));
    expect(closed).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-expanded="false"/);
    expect(closed).toContain('data-collapsible="true" data-expanded="false"');
    expect(closed).toContain('data-action="expand-token-search"');
    expect(closed).not.toContain('kui-token-search__suggestions');
    expect(closed).not.toContain('aria-label="Search syntax"');
    const open = String(TicketSearchField({ ...props, model: model('', 'tag:s'), expanded: true }));
    expect(open).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-expanded="true"/);
    expect(open).toContain('data-collapsible="true" data-expanded="true"');
    expect(open).toContain('data-token-search-suggestion="tag:server"');
    expect(open).toContain('aria-label="Search syntax"');
    const reclosed = String(TicketSearchField({ ...props, model: model(), expanded: false }));
    expect(reclosed).toBe(closed);
    // The group's expansion follows the app signal even while the populated field stays open; Kerf's
    // suggestions belong to the field itself, the app's help surface to the group's open state.
    const populatedClosed = String(TicketSearchField({ ...props, model: model('', 'tag:s'), expanded: false }));
    expect(populatedClosed).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-expanded="false"/);
    expect(populatedClosed).not.toContain('aria-label="Search syntax"');
  });

  it('passes disabled, autofocus, placeholder, and clear label through with literal root classes', () => {
    const markup = String(
      TicketSearchField({
        id: 'saved-view-query',
        label: 'Search query',
        model: model('', 'parser'),
        placeholder: 'Find tickets',
        disabled: true,
        autofocus: true,
        clearLabel: 'Clear search query',
      }),
    );
    // Only a literal class is classifiable by the Kerf analyzer; consumers pick a `layout`, never a class.
    expect(markup).toContain('class="kui-toolbar-control-group ticket-search-field"');
    expect(markup).toContain('data-token-search-id="saved-view-query" data-disabled="true"');
    expect(markup).toContain('data-placeholder="Find tickets"');
    expect(markup).toContain('aria-label="Clear search query"');
  });

  it('renders no popovers with external surfaces and lets the consumer place the same surfaces block', () => {
    const external = model('', 'updated-after:');
    const field = String(
      TicketSearchField({
        id: 'saved-view-query',
        label: 'Search query',
        model: external,
        helpOpen: true,
        surfaces: 'external',
      }),
    );
    expect(field).not.toContain('ticket-search-surfaces');
    expect(field).not.toContain('aria-label="Date and time helper"');
    expect(field).not.toContain('aria-label="Search syntax"');
    expect(field).toContain('aria-label="Search syntax help" aria-expanded="true"');
    const surfaces = String(TicketSearchSurfaces({ id: 'saved-view-query', model: external, helpOpen: true }));
    expect(surfaces).toMatch(
      /^<div class="ticket-search-surfaces" data-ticket-search-for="saved-view-query" data-token-search-keep-open>/,
    );
    expect(surfaces).toContain('role="group" aria-label="Date and time helper"');
    expect(surfaces).toContain('role="dialog" aria-label="Search syntax"');
    // The floating variant renders the identical surfaces block inside its group.
    const floating = String(
      TicketSearchField({ id: 'saved-view-query', label: 'Search query', model: external, helpOpen: true }),
    );
    expect(floating).toContain(surfaces);
    const empty = String(TicketSearchSurfaces({ id: 'saved-view-query', model: model('', 'parser') }));
    expect(empty).toBe(
      '<div class="ticket-search-surfaces" data-ticket-search-for="saved-view-query" data-token-search-keep-open></div>',
    );
    expect(String(TicketSearchSurfaces({ id: 'f', model: model(), helpOpen: true, help: false }))).not.toContain(
      'Search syntax',
    );
  });

  it('sizes itself in its toolbar through Kerf group props chosen by the layout prop (HS2-8FS5BJ, HS2-DAMHD1)', () => {
    const props = { id: 'f', label: 'Search', model: model(), collapsible: true } as const;
    const group = (markup: string): (string | undefined)[] =>
      /<div class="(kui-toolbar-control-group [^"]*)"([^>]*)>/.exec(markup) ?? [];
    const attrs = (markup: string, ...names: string[]) =>
      Object.fromEntries(
        names.map((name) => [name, new RegExp(` ${name}="([^"]*)"`).exec(group(markup)[2] ?? '')?.[1]]),
      );
    const render = (layout: 'inline' | 'grow' | 'row', expanded: boolean) =>
      String(TicketSearchField({ ...props, layout, expanded }));
    // Transition matrix: every layout closed -> open -> closed again renders Kerf's sizing, visibility,
    // and placement attributes plus the expanded state, and reclosing restores the first render exactly.
    for (const layout of ['inline', 'grow', 'row'] as const) {
      const closed = render(layout, false),
        open = render(layout, true);
      expect(attrs(closed, 'data-expanded')['data-expanded']).toBe('false');
      expect(attrs(open, 'data-expanded')['data-expanded']).toBe('true');
      expect(render(layout, false)).toBe(closed);
    }
    const kerf = (layout: 'inline' | 'grow' | 'row', expanded: boolean) =>
      attrs(render(layout, expanded), 'data-sizing', 'data-visibility', 'data-placement', 'data-expanded-overflow');
    expect(kerf('inline', true)).toEqual(kerf('inline', false));
    expect(kerf('inline', false)).toEqual(expect.objectContaining({ 'data-visibility': 'hide-collapsed-tiny' }));
    expect(kerf('grow', false)).toEqual(
      expect.objectContaining({ 'data-sizing': 'grow', 'data-visibility': 'hide-collapsed-tiny' }),
    );
    expect(kerf('grow', true)).toEqual(kerf('grow', false));
    expect(kerf('row', false)).toEqual(expect.objectContaining({ 'data-sizing': 'fill', 'data-placement': 'end' }));
    expect(kerf('row', true)).toEqual(kerf('row', false));
    // `row` hands the full-row width to Kerf's `fill` on the field too; the other layouts never fill.
    expect(render('row', true)).toContain('data-fill="true"');
    expect(render('grow', true)).toContain('data-fill="false"');
    for (const layout of ['inline', 'grow', 'row'] as const)
      for (const expanded of [false, true])
        expect(group(render(layout, expanded))[1]).toBe('kui-toolbar-control-group ticket-search-field');
    for (const layout of ['inline', 'grow', 'row'] as const)
      for (const expanded of [false, true]) expect(kerf(layout, expanded)['data-expanded-overflow']).toBe('visible');

    const css = readFileSync(new URL('./ticket-search-field.css', import.meta.url), 'utf8');
    expect(css).not.toContain('.ticket-search-field--grow-inline');
    expect(css).not.toContain('min-width: 19rem');
    expect(css).not.toContain('--kui-token-search-expanded-width');
    expect(css).not.toContain('ticket-search-field--row');
    expect(css).not.toContain(':not(.ticket-search-field--open)');
    expect(css).not.toContain('@keyframes');
  });

  it('collapses a disabled collapsible field even when its caller supplies expanded (HS2-6ZK9KF)', () => {
    const searchModel = model('is:open', 'Started');
    const markup = String(
      TicketSearchField({
        id: 'search',
        label: 'Search',
        model: searchModel,
        collapsible: true,
        expanded: true,
        disabled: true,
      }),
    );
    expect(markup).toMatch(/class="kui-toolbar-control-group ticket-search-field"[^>]*data-expanded="false"/);
    expect(markup).toContain('data-collapsible="true" data-expanded="false"');
    expect(markup).not.toContain('>Started</span>');
    expect(searchModel.state.value.query).toContain('Started');
  });

  it("gives the form field the grouped field's chip-color hook on its rendered root (HS2-RXHZVR)", () => {
    const markup = String(TicketSearchFormField({ id: 'form-search', label: 'View query', model: model('is:open') }));
    // Kerf renders `className` on the form-field root (KF-5G8WJ0), the same hook the grouped field uses.
    expect(markup).toContain(
      '<div class="kui-token-search__field ticket-search-field__query" data-token-search-form-field="form-search"',
    );
    expect(markup).toContain('data-token-value="is:open"');
    const grouped = String(TicketSearchField({ id: 'grouped', label: 'Search', model: model('is:open') }));
    expect(grouped).toContain('class="kui-token-search ticket-search-field__query"');
  });

  it('owns the token colors and helper popover styles that every consumer shares', () => {
    const css = readFileSync(new URL('./ticket-search-field.css', import.meta.url), 'utf8');
    expect(css).not.toContain('.ticket-search-field {');
    expect(css).not.toContain(".ticket-search-field[data-content='search']");
    // The chip tint is set on the app's own className hook, never on Kerf's `.kui-token-search`, and
    // the tokens that equal Kerf's defaults are not restated (HS2-8FS5BJ). Kerf's use-site token
    // fallbacks let the bare hook apply, so it needs no grouped-root qualifier and the form field
    // shares it (KF-5G8WJ0, HS2-RXHZVR).
    expect(css).toContainSource(
      '.ticket-search-field__query { --kui-token-search-token-background: var(--wa-color-brand-fill-quiet); }',
    );
    expect(css).not.toContain('.ticket-search-field .ticket-search-field__query');
    expect(css).not.toContain('.kui-');
    // Tag suggestions are Kerf's own in-flow rows now (HS2-5JXBQY); the app styles only its helpers.
    expect(css).not.toContain('__suggestions');
    expect(css).toContainSource('.ticket-search-field__date { display: grid; grid-template-columns: 1fr 1fr auto;');
    expect(css).toContainSource('.ticket-search-field > .ticket-search-surfaces { display: contents; }');
    expect(css).toContainSource(
      ':not(.ticket-search-field) > .ticket-search-surfaces { display: grid; gap: var(--wa-space-2xs); }',
    );
    expect(css).toContainSource(
      ':not(.ticket-search-field) > .ticket-search-surfaces > .ticket-search-field__date, :not(.ticket-search-field) > .ticket-search-surfaces > .ticket-search-field__help { position: static; width: 100%; max-width: 100%; box-shadow: none; }',
    );
    expect(css).toMatch(
      /\.ticket-search-field__date,\s*\.ticket-search-field__help \{[^}]*z-index: 1300;[^}]*box-sizing: border-box;/,
    );
    expect(css).toContain('@media (max-width: remify(480px))');
    expect(css).toContainSource(
      '.ticket-search-field__date, .ticket-search-field__help { right: auto; left: 0; width: min(remify(304px), calc(100vw - 2 * var(--wa-space-s))); }',
    );
    expect(css).toContainSource('.ticket-search-field__date { grid-template-columns: repeat(2, minmax(0, 1fr)); }');
    expect(css).toContainSource('.ticket-search-field__date input { width: 100%; min-width: 0; }');
    expect(css).toContainSource('.ticket-search-field__date button { grid-column: 1 / -1; }');
    expect(css).toContainSource(
      '.ticket-search-field__help code { max-width: 100%; white-space: normal; overflow-wrap: anywhere; }',
    );
    expect(css).toContainSource(
      '.ticket-search-field__help { max-height: min(remify(448px), calc(100dvh - remify(384px))); }',
    );
    expect(css).toContainSource(
      '@media (max-height: remify(704px)) { .ticket-search-field__help { max-height: calc(100vh - remify(368px)); } }',
    );
    expect(css).not.toContain('workspace-header');
  });
});
