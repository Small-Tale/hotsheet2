import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { tokenFromRaw } from '../inline-search';
import { TICKET_SEARCH_ACTIONS, TicketSearchField, TicketSearchSurfaces } from './ticket-search-field';

const tags = ['client', 'Docs', 'needs design', 'server'];

describe('TicketSearchField (HS2-N5G6JS)', () => {
  it("composes Kerf's grouped token field with generic, id-resolved actions", () => {
    const markup = String(
      TicketSearchField({
        id: 'demo-search',
        label: 'Search tickets',
        query: 'parser',
        tokens: [tokenFromRaw('is:open')!],
      }),
    );
    expect(markup).toContain('class="kui-toolbar-control-group ticket-search-field"');
    expect(markup).toMatch(/ticket-search-field"[^>]*data-expanded="true"/);
    expect(markup).toContain('data-content="search"');
    expect(markup).toContain('data-component="token-search-field" data-token-search-id="demo-search"');
    expect(markup).toContain('role="searchbox" aria-label="Search tickets"');
    expect(markup).toContain('data-token-value="is:open"');
    expect(markup).toContain(`data-action="${TICKET_SEARCH_ACTIONS.editToken}"`);
    expect(markup).toContain(`data-action="${TICKET_SEARCH_ACTIONS.removeToken}"`);
    expect(markup).toContain(`data-action="${TICKET_SEARCH_ACTIONS.clear}"`);
    expect(markup).toContain('aria-label="Clear search"');
    expect(markup).toMatch(
      new RegExp(
        `class="ticket-search-field__help-button" data-action="${TICKET_SEARCH_ACTIONS.toggleHelp}" aria-label="Search syntax help" aria-expanded="false"`,
      ),
    );
    expect(markup).toContain('data-lucide="circle-help"');
    expect(markup).toContain('data-collapsible="false" data-expanded="true"');
    expect(markup).not.toContain('aria-label="Matching tags"');
    expect(markup).not.toContain('aria-label="Date and time helper"');
    expect(markup).not.toContain('aria-label="Search syntax"');
    expect(markup).not.toContain('workspace-search');
  });

  it('derives in-place tag suggestions from the project tags and the trailing tag: text', () => {
    const suggest = (query: string, tokens = [tokenFromRaw('tag:client')!]) =>
      [
        ...String(TicketSearchField({ id: 'f', label: 'Search', query, tokens, tags })).matchAll(/data-tag="([^"]+)"/g),
      ].map((match) => match[1]);
    // Case-insensitive prefix, committed chips excluded, quoted display for spaces.
    expect(suggest('parser tag:')).toEqual(['Docs', 'needs design', 'server']);
    expect(suggest('tag:D')).toEqual(['Docs']);
    expect(suggest('tag:"nee')).toEqual(['needs design']);
    expect(suggest('tag:cl')).toEqual([]);
    expect(suggest('tag:cl', [])).toEqual(['client']);
    expect(suggest('tag:client parser')).toEqual([]);
    expect(suggest('')).toEqual([]);
    const markup = String(TicketSearchField({ id: 'f', label: 'Search', query: 'tag:n', tags }));
    expect(markup).toContain('role="listbox" aria-label="Matching tags"');
    expect(markup).toContain(
      `<button type="button" role="option" data-action="${TICKET_SEARCH_ACTIONS.selectTag}" data-tag="needs design">tag:&quot;needs design&quot;</button>`,
    );
  });

  it('offers the date helper for a trailing lifecycle filter and the syntax help on request', () => {
    const date = String(TicketSearchField({ id: 'f', label: 'Search', query: 'updated-after:' }));
    expect(date).toContain('role="group" aria-label="Date and time helper"');
    expect(date).toContain('name="ticket-search-date" type="date"');
    expect(date).toContain('name="ticket-search-time" type="time"');
    expect(date).toContain(`data-action="${TICKET_SEARCH_ACTIONS.applyDate}" data-date-prefix="updated-after"`);
    expect(date).not.toContain('aria-label="Search syntax"');
    const help = String(TicketSearchField({ id: 'f', label: 'Search', helpOpen: true }));
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
    const noHelp = String(TicketSearchField({ id: 'f', label: 'Search', helpOpen: true, help: false }));
    expect(noHelp).not.toContain('Search syntax');
  });

  it('collapses every helper surface with a closed collapsible field and reopens them with it', () => {
    // Kerf keeps a populated collapsible field open, so the closed state is the empty one.
    const props = { id: 'f', label: 'Search', tags, helpOpen: true, collapsible: true } as const;
    const closed = String(TicketSearchField({ ...props, query: '', expanded: false }));
    expect(closed).toMatch(/ticket-search-field"[^>]*data-expanded="false"/);
    expect(closed).toContain('data-collapsible="true" data-expanded="false"');
    expect(closed).toContain('data-action="expand-token-search"');
    expect(closed).not.toContain('aria-label="Matching tags"');
    expect(closed).not.toContain('aria-label="Search syntax"');
    const open = String(TicketSearchField({ ...props, query: 'tag:s', expanded: true }));
    expect(open).toMatch(/ticket-search-field"[^>]*data-expanded="true"/);
    expect(open).toContain('data-collapsible="true" data-expanded="true"');
    expect(open).toContain('data-tag="server"');
    expect(open).toContain('aria-label="Search syntax"');
    const reclosed = String(TicketSearchField({ ...props, query: '', expanded: false }));
    expect(reclosed).toBe(closed);
    // The group's expansion follows the app signal even while the populated field stays open.
    const populatedClosed = String(TicketSearchField({ ...props, query: 'tag:s', expanded: false }));
    expect(populatedClosed).toMatch(/ticket-search-field"[^>]*data-expanded="false"/);
    expect(populatedClosed).not.toContain('aria-label="Matching tags"');
  });

  it('passes disabled, autofocus, placeholder, revision, and clear label through with one static root class', () => {
    const markup = String(
      TicketSearchField({
        id: 'saved-view-query',
        label: 'Search query',
        query: 'parser',
        placeholder: 'Find tickets',
        disabled: true,
        autofocus: true,
        revision: 3,
        clearLabel: 'Clear search query',
      }),
    );
    // Only a literal class is classifiable by the Kerf analyzer; consumers place the group by context.
    expect(markup).toContain('class="kui-toolbar-control-group ticket-search-field"');
    expect(markup).toContain('data-token-search-id="saved-view-query" data-disabled="true"');
    expect(markup).toContain('data-placeholder="Find tickets"');
    expect(markup).toContain('aria-label="Clear search query"');
  });

  it('renders no popovers with external surfaces and lets the consumer place the same surfaces block', () => {
    const field = String(
      TicketSearchField({
        id: 'saved-view-query',
        label: 'Search query',
        query: 'tag:s',
        tags,
        helpOpen: true,
        surfaces: 'external',
      }),
    );
    expect(field).not.toContain('ticket-search-surfaces');
    expect(field).not.toContain('aria-label="Matching tags"');
    expect(field).not.toContain('aria-label="Search syntax"');
    expect(field).toContain('aria-label="Search syntax help" aria-expanded="true"');
    const surfaces = String(TicketSearchSurfaces({ id: 'saved-view-query', query: 'tag:s', tags, helpOpen: true }));
    expect(surfaces).toMatch(
      /^<div class="ticket-search-surfaces" data-ticket-search-for="saved-view-query" data-token-search-keep-open>/,
    );
    expect(surfaces).toContain('role="listbox" aria-label="Matching tags"');
    expect(surfaces).toContain('data-tag="server"');
    expect(surfaces).toContain('role="dialog" aria-label="Search syntax"');
    // The floating variant renders the identical surfaces block inside its group.
    const floating = String(
      TicketSearchField({ id: 'saved-view-query', label: 'Search query', query: 'tag:s', tags, helpOpen: true }),
    );
    expect(floating).toContain(surfaces);
    const empty = String(TicketSearchSurfaces({ id: 'saved-view-query', query: 'parser', tags }));
    expect(empty).toBe(
      '<div class="ticket-search-surfaces" data-ticket-search-for="saved-view-query" data-token-search-keep-open></div>',
    );
    expect(String(TicketSearchSurfaces({ id: 'f', helpOpen: true, help: false }))).not.toContain('Search syntax');
  });

  it('owns the token colors and helper popover styles that every consumer shares', () => {
    const css = readFileSync(new URL('./ticket-search-field.css', import.meta.url), 'utf8');
    expect(css).toContainSource('.ticket-search-field { position: relative; }');
    expect(css).toContainSource(
      ".ticket-search-field[data-content='search'][data-expanded='true'] { height: auto; overflow: visible; }",
    );
    expect(css).toContainSource(
      '.ticket-search-field .kui-token-search { --kui-token-search-background: var(--wa-color-surface-default); --kui-token-search-border: var(--wa-color-neutral-border-normal); --kui-token-search-token-background: var(--wa-color-brand-fill-quiet); --kui-token-search-token-foreground: var(--wa-color-brand-on-quiet); }',
    );
    expect(css).toContainSource(
      '.ticket-search-field__suggestions{display:flex;box-sizing:border-box;width:min(remify(416px),100%);align-items:stretch;flex-direction:column;text-align:left}',
    );
    expect(css).toContainSource(
      '.ticket-search-field__suggestions button{display:block;box-sizing:border-box;width:100%;',
    );
    expect(css).toContainSource('.ticket-search-field__date { display: grid; grid-template-columns: 1fr 1fr auto;');
    expect(css).toContainSource('.ticket-search-field > .ticket-search-surfaces { display: contents; }');
    expect(css).toContainSource(
      ':not(.ticket-search-field) > .ticket-search-surfaces { display: grid; gap: var(--wa-space-2xs); }',
    );
    expect(css).toContainSource(
      ':not(.ticket-search-field) > .ticket-search-surfaces > .ticket-search-field__suggestions, :not(.ticket-search-field) > .ticket-search-surfaces > .ticket-search-field__date, :not(.ticket-search-field) > .ticket-search-surfaces > .ticket-search-field__help { position: static; width: 100%; max-width: 100%; box-shadow: none; }',
    );
    expect(css).toMatch(
      /\.ticket-search-field__suggestions,\s*\.ticket-search-field__date,\s*\.ticket-search-field__help \{[^}]*z-index: 1300;[^}]*box-sizing: border-box;/,
    );
    expect(css).toContainSource(
      '@media (max-height: remify(704px)) { .ticket-search-field__help { max-height: calc(100vh - remify(368px)); } }',
    );
    expect(css).not.toContain('workspace-header');
  });
});
