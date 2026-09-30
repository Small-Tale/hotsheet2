import { Toolbar } from '@kerfjs/ui/toolbar';
import { batch, type Signal, signal } from 'kerfjs';

import { TicketSearchField, TicketSearchSurfaces } from '../components/ticket-search-field';
import {
  ACTIVE_TAG_PATTERN,
  activeDatePattern,
  consumeSearchTokens,
  type InlineSearchToken,
  type SearchDatePrefix,
  tagSearchToken,
} from '../inline-search';

/** Deterministic project tags for the TicketSearchField catalog entry. */
export const TICKET_SEARCH_DEMO_TAGS = ['client', 'docs', 'needs design', 'parser', 'server', 'ui'] as const;

export interface TicketSearchDemoField {
  readonly query: Signal<string>;
  readonly tokens: Signal<InlineSearchToken[]>;
  readonly helpOpen: Signal<boolean>;
}

const createField = (): TicketSearchDemoField => ({
  query: signal(''),
  tokens: signal<InlineSearchToken[]>([]),
  helpOpen: signal(false),
});

/** Demo state for the two non-collapsible fields, keyed by their editor id. */
export const ticketSearchDemoFields = {
  'ticket-search-demo': createField(),
  'ticket-search-demo-external': createField(),
} as const;
export const ticketSearchDemoCollapsibleOpen = signal(false);
export const ticketSearchDemoCollapsibleQuery = signal('');
export const ticketSearchDemoEvent = signal('');

export function ticketSearchDemoField(id: string): TicketSearchDemoField | undefined {
  return Object.hasOwn(ticketSearchDemoFields, id)
    ? ticketSearchDemoFields[id as keyof typeof ticketSearchDemoFields]
    : undefined;
}

export function resetTicketSearchDemo(): void {
  batch(() => {
    for (const field of Object.values(ticketSearchDemoFields)) {
      field.query.value = '';
      field.tokens.value = [];
      field.helpOpen.value = false;
    }
    ticketSearchDemoCollapsibleOpen.value = false;
    ticketSearchDemoCollapsibleQuery.value = '';
    ticketSearchDemoEvent.value = '';
  });
}

function addDemoToken(field: TicketSearchDemoField, token: InlineSearchToken, offset = field.query.value.length) {
  if (!field.tokens.value.some((value) => value.kind === token.kind && value.value === token.value))
    field.tokens.value = [...field.tokens.value, { ...token, offset }];
  field.helpOpen.value = false;
}

/** Mirror of the application's trailing-filter replacement for a demo field. */
export function replaceActiveDemoToken(id: string, pattern: RegExp, token: InlineSearchToken): void {
  const field = ticketSearchDemoField(id);
  if (!field) return;
  const match = field.query.value.match(pattern);
  if (match) {
    const raw = match[1],
      start = match.index! + match[0].lastIndexOf(raw);
    batch(() => {
      field.query.value = field.query.value.slice(0, start) + field.query.value.slice(start + raw.length);
      addDemoToken(field, token, start);
    });
  } else addDemoToken(field, token);
  ticketSearchDemoEvent.value = `Added ${token.raw}`;
}

export function selectDemoTag(id: string, tag: string): void {
  const token = tagSearchToken(tag, TICKET_SEARCH_DEMO_TAGS);
  if (token) replaceActiveDemoToken(id, ACTIVE_TAG_PATTERN, token);
}

export function applyDemoDate(id: string, prefix: SearchDatePrefix, token: InlineSearchToken): void {
  replaceActiveDemoToken(id, activeDatePattern(prefix), token);
}

export function toggleDemoHelp(id: string): void {
  const field = ticketSearchDemoField(id);
  if (field) field.helpOpen.value = !field.helpOpen.value;
}

/** Commit whitespace-terminated filters typed into a demo field as chips. */
export function editDemoQuery(id: string, text: string, force = false): void {
  const field = ticketSearchDemoField(id);
  if (!field) return;
  const parsed = consumeSearchTokens(text, force);
  batch(() => {
    field.query.value = parsed.text;
    for (const token of parsed.tokens) addDemoToken(field, token);
  });
  if (parsed.tokens.length)
    ticketSearchDemoEvent.value = `Committed ${parsed.tokens.map((token) => token.raw).join(' ')}`;
}

export function removeDemoToken(id: string, raw: string): void {
  const field = ticketSearchDemoField(id),
    token = field?.tokens.value.find((value) => value.raw === raw);
  if (!field || !token) return;
  field.tokens.value = field.tokens.value.filter((value) => value !== token);
  ticketSearchDemoEvent.value = `Removed ${raw}`;
}

export function editDemoToken(id: string, raw: string): void {
  const field = ticketSearchDemoField(id),
    token = field?.tokens.value.find((value) => value.raw === raw);
  if (!field || !token) return;
  batch(() => {
    field.tokens.value = field.tokens.value.filter((value) => value !== token);
    field.query.value = `${field.query.value.trimEnd()} ${raw}`.trimStart();
  });
  ticketSearchDemoEvent.value = `Editing ${raw}`;
}

export function clearDemoQuery(id: string): void {
  const field = ticketSearchDemoField(id);
  if (!field) return;
  batch(() => {
    field.query.value = '';
    field.tokens.value = [];
    field.helpOpen.value = false;
  });
  ticketSearchDemoEvent.value = 'Cleared';
}

export function TicketSearchFieldDemo() {
  const floating = ticketSearchDemoFields['ticket-search-demo'],
    external = ticketSearchDemoFields['ticket-search-demo-external'];
  return (
    <section class="ticket-search-field-demo" aria-label="TicketSearchField demo">
      <div>
        <h2>Standalone query field, floating surfaces</h2>
        <p class="component-stage__hint">
          Type <code>tag:</code> for in-place tag completion, a lifecycle filter such as <code>updated-after:</code> for
          the date helper, or open the syntax help. Whitespace commits a filter as a chip.
        </p>
        <Toolbar
          className="ticket-search-field-demo__toolbar"
          centerAlign="stretch"
          center={
            <TicketSearchField
              id="ticket-search-demo"
              label="Search query"
              query={floating.query.value}
              tokens={floating.tokens.value}
              tags={TICKET_SEARCH_DEMO_TAGS}
              helpOpen={floating.helpOpen.value}
              clearLabel="Clear search query"
            />
          }
        />
      </div>
      <div class="ticket-search-field-demo__external">
        <h2>External surfaces, dialog layout</h2>
        <p class="component-stage__hint">
          Inside a dialog or other clipping container the field renders no popovers; the consumer places
          <code>TicketSearchSurfaces</code> for the same id in its own stacked layout.
        </p>
        <Toolbar
          className="ticket-search-field-demo__toolbar"
          centerAlign="stretch"
          center={
            <TicketSearchField
              id="ticket-search-demo-external"
              label="Dialog search query"
              query={external.query.value}
              tokens={external.tokens.value}
              tags={TICKET_SEARCH_DEMO_TAGS}
              helpOpen={external.helpOpen.value}
              clearLabel="Clear dialog search query"
              surfaces="external"
            />
          }
        />
        <TicketSearchSurfaces
          id="ticket-search-demo-external"
          query={external.query.value}
          tokens={external.tokens.value}
          tags={TICKET_SEARCH_DEMO_TAGS}
          helpOpen={external.helpOpen.value}
        />
      </div>
      <div>
        <h2>Collapsible toolbar field</h2>
        <Toolbar
          className="ticket-search-field-demo__toolbar ticket-search-field-demo__collapsible-toolbar"
          trailing={
            <TicketSearchField
              id="ticket-search-demo-collapsible"
              label="Search tickets"
              query={ticketSearchDemoCollapsibleQuery.value}
              tags={TICKET_SEARCH_DEMO_TAGS}
              collapsible
              expanded={ticketSearchDemoCollapsibleOpen.value}
            />
          }
        />
      </div>
      <div>
        <h2>Disabled</h2>
        <Toolbar
          className="ticket-search-field-demo__toolbar"
          centerAlign="stretch"
          center={<TicketSearchField id="ticket-search-demo-disabled" label="Search query" query="is:open" disabled />}
        />
      </div>
      <p class="component-stage__event" aria-live="polite">
        {ticketSearchDemoEvent.value}
      </p>
    </section>
  );
}
