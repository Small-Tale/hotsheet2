import { CatalogExample } from '@kerfjs/ui/catalog';
import { Grid } from '@kerfjs/ui/grid';
import type { TokenSearchModel } from '@kerfjs/ui/token-search-model';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarText } from '@kerfjs/ui/toolbar-text';
import { batch, signal } from 'kerfjs';

import { TicketSearchField, TicketSearchFormField } from '../components/ticket-search-field';
import { createTicketSearchModel, replaceTicketSearch } from '../ticket-search-model';
import { DemoEvent } from './components/demo-event';
import { onDemoSelected } from './components/demo-state';
import type { DemoModule } from './demo-module';

/** Deterministic project tags for the TicketSearchField catalog entry. */
export const TICKET_SEARCH_DEMO_TAGS = ['client', 'docs', 'needs design', 'parser', 'server', 'ui'] as const;

export const ticketSearchDemoCollapsibleOpen = signal(false);
/** Open state of the `layout="grow"` and `layout="row"` collapsible specimens (HS2-8FS5BJ). */
export const ticketSearchDemoGrowOpen = signal(false);
export const ticketSearchDemoRowOpen = signal(false);
export const ticketSearchDemoEvent = signal('');
const ticketSearchDemoHelpOpen: Record<string, ReturnType<typeof signal<boolean>>> = {
  'ticket-search-demo': signal(false),
  'ticket-search-demo-form': signal(false),
  // The SavedViewDialog catalog entry's query field shares the demo help routing (HS2-DF6N61).
  'saved-view-query': signal(false),
};

/** Whether a demo search field's syntax help is open. */
export function ticketSearchDemoHelpIsOpen(id: string): boolean {
  return Object.hasOwn(ticketSearchDemoHelpOpen, id) ? ticketSearchDemoHelpOpen[id].value : false;
}

/**
 * A Kerf-managed demo model whose actions also narrate the catalog's event line, so the demo
 * proves which model action each control reaches (HS2-N5G6JS, HS2-5JXBQY).
 */
function demoModel(initial = '', { commit = true } = {}): TokenSearchModel {
  const model = createTicketSearchModel({ tags: () => TICKET_SEARCH_DEMO_TAGS });
  if (initial && commit) replaceTicketSearch(model, initial);
  else if (initial) model.replace({ query: initial, tokens: [] });
  const chips = () => model.state.value.tokens.map((token) => token.value);
  return {
    ...model,
    edit(value, commit) {
      const before = chips();
      model.edit(value, commit);
      const added = chips().filter((chip) => !before.includes(chip));
      if (added.length) ticketSearchDemoEvent.value = `Committed ${added.join(' ')}`;
    },
    choose(value) {
      const before = chips();
      model.choose(value);
      const added = chips().filter((chip) => !before.includes(chip));
      if (added.length) ticketSearchDemoEvent.value = `Added ${added.join(' ')}`;
    },
    commit(value) {
      const before = chips();
      model.commit(value);
      const added = chips().filter((chip) => !before.includes(chip));
      if (added.length) ticketSearchDemoEvent.value = `Added ${added.join(' ')}`;
    },
    expandToken(value) {
      model.expandToken(value);
      ticketSearchDemoEvent.value = `Editing ${value}`;
    },
    remove(value) {
      model.remove(value);
      ticketSearchDemoEvent.value = `Removed ${value}`;
    },
    clear() {
      model.clear();
      ticketSearchDemoEvent.value = 'Cleared';
    },
  };
}

/** The demo fields' models, keyed by editor id for `wireTokenSearchFields({ models })`. */
export const ticketSearchDemoModels: Readonly<Record<string, TokenSearchModel>> = {
  'ticket-search-demo': demoModel(),
  'ticket-search-demo-collapsible': demoModel(),
  'ticket-search-demo-grow': demoModel(),
  'ticket-search-demo-row': demoModel(),
  'ticket-search-demo-form': demoModel(),
  // The disabled specimen shows an uncommitted filter as plain text, not a chip.
  'ticket-search-demo-disabled': demoModel('is:open', { commit: false }),
};
/** The saved-view dialog catalog entry's seeded query model. */
export const savedViewDemoSearchModel = demoModel('is:open tag:bug');

export function ticketSearchDemoModel(id: string): TokenSearchModel | undefined {
  if (id === 'saved-view-query') return savedViewDemoSearchModel;
  return Object.hasOwn(ticketSearchDemoModels, id) ? ticketSearchDemoModels[id] : undefined;
}

export function resetTicketSearchDemo(): void {
  batch(() => {
    for (const [id, model] of Object.entries(ticketSearchDemoModels))
      if (id === 'ticket-search-demo-disabled') model.replace({ query: 'is:open', tokens: [] });
      else model.replace({ query: '', tokens: [] });
    for (const open of Object.values(ticketSearchDemoHelpOpen)) open.value = false;
    ticketSearchDemoCollapsibleOpen.value = false;
    ticketSearchDemoGrowOpen.value = false;
    ticketSearchDemoRowOpen.value = false;
    ticketSearchDemoEvent.value = '';
  });
}

export function toggleDemoHelp(id: string): void {
  const open = Object.hasOwn(ticketSearchDemoHelpOpen, id) ? ticketSearchDemoHelpOpen[id] : undefined;
  if (open) open.value = !open.value;
}

export function TicketSearchFieldDemo() {
  const floating = ticketSearchDemoModels['ticket-search-demo'];
  return (
    <>
      <CatalogExample
        align="glyph"
        label="Standalone query field, floating surfaces"
        note={
          <>
            Type <code>tag:</code> for Kerf's in-place tag completion, a lifecycle filter such as{' '}
            <code>updated-after:</code> for the date helper, or open the syntax help. Whitespace commits a filter as a
            chip.
          </>
        }
      >
        <Toolbar
          centerAlign="stretch"
          center={
            <TicketSearchField
              id="ticket-search-demo"
              label="Search query"
              model={floating}
              helpOpen={ticketSearchDemoHelpOpen['ticket-search-demo'].value}
              clearLabel="Clear search query"
            />
          }
        />
      </CatalogExample>
      <CatalogExample
        align="glyph"
        label="Form field beside Web Awesome inputs"
        rootAttributes={{ 'data-search-demo': 'form' }}
        note={
          <>
            <code>TicketSearchFormField</code> uses Kerf's form-field presentation: a visible label, hint, and required
            marker that line up with a <code>wa-input</code>, with the same anchored helper surfaces (HS2-E40KC0,
            HS2-8TE69P).
          </>
        }
        viewport={{ width: 'full' }}
      >
        <Grid columns={1} gap="m" textInsets="rl">
          <wa-input name="ticket-search-demo-form-name" label="View name" required />
          <TicketSearchFormField
            id="ticket-search-demo-form"
            label="View query"
            model={ticketSearchDemoModels['ticket-search-demo-form']}
            required
            hint="Use the same words, fields, operators, and filter chips as ticket search."
            helpOpen={ticketSearchDemoHelpOpen['ticket-search-demo-form'].value}
            clearLabel="Clear form search query"
          />
        </Grid>
      </CatalogExample>
      <CatalogExample
        align="glyph"
        label="Collapsible toolbar field, inline layout"
        note={
          <>
            <code>layout="inline"</code> (the default) keeps Kerf's own collapsed and expanded widths; a{' '}
            <code>trailing-priority</code> Toolbar reserves the expanded field's track.
          </>
        }
      >
        <Toolbar
          responsive="trailing-priority"
          responsiveAt="narrow"
          trailing={
            <TicketSearchField
              id="ticket-search-demo-collapsible"
              label="Search tickets"
              model={ticketSearchDemoModels['ticket-search-demo-collapsible']}
              collapsible
              expanded={ticketSearchDemoCollapsibleOpen.value}
            />
          }
        />
      </CatalogExample>
      <CatalogExample
        align="glyph"
        label="Grow layout (workspace header)"
        note={
          <>
            <code>layout="grow"</code>: the open field grows into the free room on its row from a 19rem floor, takes the
            whole row on a compact toolbar, and its collapsed icon leaves the tiniest toolbars (HS2-8FS5BJ).
          </>
        }
      >
        <Toolbar
          responsive="stack"
          responsiveAt="compact"
          leading={<ToolbarText text="Queue" size="large" />}
          trailing={
            <TicketSearchField
              id="ticket-search-demo-grow"
              label="Search grow layout"
              model={ticketSearchDemoModels['ticket-search-demo-grow']}
              collapsible
              expanded={ticketSearchDemoGrowOpen.value}
              layout="grow"
            />
          }
        />
      </CatalogExample>
      <CatalogExample
        align="glyph"
        label="Row layout (narrow rail)"
        note={
          <>
            <code>layout="row"</code>: the collapsed icon sits at its zone's trailing edge and the open field fills a
            row of its own (Kerf <code>fill</code>), sliding in from the row above.
          </>
        }
        viewport={{ width: 'text' }}
      >
        <Toolbar
          responsive="stack"
          responsiveAt="narrow"
          leading={<ToolbarText text="Queue" size="large" />}
          trailing={
            <TicketSearchField
              id="ticket-search-demo-row"
              label="Search row layout"
              model={ticketSearchDemoModels['ticket-search-demo-row']}
              collapsible
              expanded={ticketSearchDemoRowOpen.value}
              layout="row"
            />
          }
        />
      </CatalogExample>
      <CatalogExample align="glyph" label="Disabled">
        <Toolbar
          centerAlign="stretch"
          center={
            <TicketSearchField
              id="ticket-search-demo-disabled"
              label="Search query"
              model={ticketSearchDemoModels['ticket-search-demo-disabled']}
              disabled
            />
          }
        />
      </CatalogExample>
      <DemoEvent>{ticketSearchDemoEvent.value}</DemoEvent>
    </>
  );
}

/** Leaving the TicketSearchField entry restores its fixtures (HS2-HMVBZW). */
function wireTicketSearchFieldDemo(): () => void {
  return onDemoSelected((id) => {
    if (id !== 'ticket-search-field') resetTicketSearchDemo();
  });
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'ticket-search-field': { catalogExamples: true, Demo: TicketSearchFieldDemo },
  },
  wire: wireTicketSearchFieldDemo,
};
