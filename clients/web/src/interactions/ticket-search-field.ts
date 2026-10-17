import { openAnchoredSurface } from '@kerfjs/ui/anchored-surface';
import { placeSearchFieldCaret } from '@kerfjs/ui/search-field';
import { type SearchFieldModel } from '@kerfjs/ui/search-field-model';
import { delegate } from 'kerfjs';
import { type OverlayHandle } from 'kerfjs/overlay';
import { createScope } from 'kerfjs/scope';

import {
  TICKET_SEARCH_ACTIONS,
  TICKET_SEARCH_DATE_INPUT,
  TICKET_SEARCH_TIME_INPUT,
  TicketSearchDateHelper,
  ticketSearchFieldId,
  TicketSearchHelp,
} from '../components/ticket-search-field';
import { dateTokenFromInput, type SearchDatePrefix } from '../inline-search';
import { data } from './dom';
import { type InteractionTeardown } from './lifetime';

/**
 * Per-field callbacks for the actions every TicketSearchField renders. Each receives the owning
 * field id so one registration serves the workspace search, the workspace-grid rail, the
 * saved-view dialog, and any later ticket-search surface (HS2-N5G6JS). Chip edit, removal, clear,
 * and tag completion are Kerf's managed model actions (HS2-5JXBQY): the callbacks here run
 * beside them, for focus and the app's own helper surfaces.
 */
export interface TicketSearchFieldHandlers {
  /**
   * The date helper produced a value for the trailing lifecycle filter (`YYYY-MM-DD` or
   * `YYYY-MM-DDTHH:MM`); the owner commits it through its model for the active prefix.
   */
  readonly applyDate: (id: string, prefix: SearchDatePrefix, value: string) => void;
  readonly toggleHelp: (id: string) => void;
  /**
   * A syntax-help example chip was chosen: add `token` to the field's search, keep its other
   * tokens, close the help, and return focus to the field (HS2-FR1Y70).
   */
  readonly insertSyntax: (id: string, token: string) => void;
  /** Kerf's managed clear emptied the model; close the app's helper surfaces and restore focus. */
  readonly clear: (id: string) => void;
  /** A chip's remove button was pressed (Kerf removes it); restore the caret where the chip sat. */
  readonly removeToken: (id: string, raw: string) => void;
  /** A chip's edit button was pressed (Kerf expands it to text); place the caret after that text. */
  readonly editToken: (id: string, raw: string) => void;
}

/**
 * Add a syntax-help example to a field's search (HS2-FR1Y70): append it as typed text and let the
 * model commit it, so a filter becomes a chip beside the existing ones, plain words stay words,
 * and existing tokens (anchored at earlier offsets) are kept.
 */
export function insertSearchSyntax(model: Pick<SearchFieldModel, 'state' | 'edit'>, token: string) {
  const { query, tokens } = model.state.value;
  model.edit({ query: `${query.trimEnd()}${query.trim() ? ' ' : ''}${token} `, tokens }, true);
}

/** The element whose descendants belong to one field: its group or form-field wrapper. */
function withField(root: HTMLElement, target: Element, run: (id: string) => void) {
  const id = ticketSearchFieldId(target),
    scope = target.closest<HTMLElement>('.ticket-search-field, .ticket-search-form-field');
  if (id && scope && root.contains(scope)) run(id);
}

/** Read the date helper's native date/time inputs and apply them. */
function applyDate(id: string, surface: Element, handlers: TicketSearchFieldHandlers) {
  const date = surface.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_DATE_INPUT}"]`)?.value;
  if (!date) return;
  const time = surface.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_TIME_INPUT}"]`)?.value ?? '',
    prefix = surface.querySelector<HTMLElement>(TICKET_SEARCH_ACTIONS.applyDate.selector)?.dataset.datePrefix as
      SearchDatePrefix | undefined,
    token = prefix ? dateTokenFromInput(prefix, date, time, navigator.language) : undefined;
  if (prefix && token) handlers.applyDate(id, prefix, `${date}${time ? `T${time}` : ''}`);
}

interface OpenSurface {
  readonly handle: OverlayHandle;
  readonly anchor: Element;
  readonly variant: string;
}

/**
 * Keep each field's date helper and syntax help in Kerf anchored surfaces that match the
 * `data-ticket-search-surface-state` marker its TicketSearchField renders (HS2-8TE69P). The
 * surfaces hang off the field's group (or form-field wrapper), live in the top layer or the
 * enclosing `wa-dialog`'s host slot (KF-F52T5D), and never take focus, so typing stays in the
 * editor. Their wrappers carry `data-search-field-keep-open`, so pressing inside one does not
 * collapse the field. The date helper follows the query and closes only through state; help
 * also closes on Escape or an outside press, routed through `toggleHelp`.
 */
function wireTicketSearchSurfaces(root: HTMLElement, handlers: TicketSearchFieldHandlers): InteractionTeardown {
  const open = new Map<string, OpenSurface>();
  const close = (key: string) => {
    const surface = open.get(key);
    if (!surface) return;
    open.delete(key);
    surface.handle.close();
  };
  const openDate = (id: string, anchor: Element, prefix: string): OverlayHandle => {
    const handle = openAnchoredSurface(anchor, TicketSearchDateHelper({ id, prefix }), {
      label: 'Search date helper',
      placement: 'bottom',
      // Hang from the field's end edge; Kerf keeps an 8px viewport gutter and fits the height to
      // the room beside the field, scrolling the surface (KF-K4GE6K, HS2-GZBJQP).
      align: 'end',
      initialFocus: false,
      dismiss: false,
    });
    handle.el.setAttribute('data-search-field-keep-open', '');
    handle.el.addEventListener('click', (event) => {
      if ((event.target as Element).closest(TICKET_SEARCH_ACTIONS.applyDate.selector))
        applyDate(id, handle.el, handlers);
    });
    // Enter inside the date helper applies it instead of submitting an enclosing form.
    handle.el.addEventListener('keydown', (event) => {
      if (
        event.key !== 'Enter' ||
        !(event.target as Element).closest(
          `input[name="${TICKET_SEARCH_DATE_INPUT}"], input[name="${TICKET_SEARCH_TIME_INPUT}"]`,
        )
      )
        return;
      event.preventDefault();
      applyDate(id, handle.el, handlers);
    });
    return handle;
  };
  const openHelp = (id: string, anchor: Element, key: string): OverlayHandle => {
    const date = open.get(`date:${id}`)?.handle.el,
      handle = openAnchoredSurface(anchor, TicketSearchHelp({ id }), {
        label: 'Search syntax',
        placement: 'bottom',
        align: 'end',
        initialFocus: false,
        dismiss: ['escape', 'outside'],
        // The field's own controls (help toggle, clear, chips) and its date helper are not outside.
        outsideIgnore: date ? [anchor, date] : anchor,
        onDismiss: () => {
          if (open.get(key)?.handle !== handle) return;
          open.delete(key);
          handlers.toggleHelp(id);
        },
      });
    handle.el.setAttribute('data-search-field-keep-open', '');
    // Example chips are native buttons, so a click covers pointer, Enter, and Space (HS2-FR1Y70).
    handle.el.addEventListener('click', (event) => {
      const chip = (event.target as Element).closest<HTMLElement>(TICKET_SEARCH_ACTIONS.insertSyntax.selector);
      const token = chip?.dataset.itemId;
      if (token) handlers.insertSyntax(id, token);
    });
    return handle;
  };
  const sync = () => {
    const wanted = new Set<string>();
    for (const marker of root.querySelectorAll<HTMLElement>('[data-ticket-search-surface-state]')) {
      const id = marker.dataset.ticketSearchFor,
        anchor = marker.closest('.ticket-search-field, .ticket-search-form-field');
      if (!id || !anchor) continue;
      const prefix = marker.dataset.surfacesOpen === 'true' ? (marker.dataset.datePrefix ?? '') : '',
        surfaces: [string, string, () => OverlayHandle][] = [];
      if (prefix) surfaces.push([`date:${id}`, prefix, () => openDate(id, anchor, prefix)]);
      if (marker.dataset.helpOpen === 'true')
        surfaces.push([`help:${id}`, 'help', () => openHelp(id, anchor, `help:${id}`)]);
      for (const [key, variant, create] of surfaces) {
        wanted.add(key);
        const current = open.get(key);
        if (current?.anchor === anchor && current.variant === variant) continue;
        close(key);
        open.set(key, { handle: create(), anchor, variant });
      }
    }
    for (const key of [...open.keys()]) if (!wanted.has(key)) close(key);
  };
  // Re-sync after every render under the root; the markers are the only state read.
  const observer = new MutationObserver(sync);
  observer.observe(root, {
    subtree: true,
    childList: true,
    attributes: true,
    attributeFilter: ['data-date-prefix', 'data-surfaces-open', 'data-help-open'],
  });
  sync();
  return () => {
    observer.disconnect();
    for (const key of [...open.keys()]) close(key);
  };
}

/**
 * Wire the delegated actions of every TicketSearchField under `root`. Kerf's
 * `wireSearchFields` owns editor chrome, submit, collapsible behavior, and (with the
 * registered models) parsing, chips, suggestions, edit, removal, and clear; this adds the Hot
 * Sheet helper surfaces (date helper, syntax help), focus restoration around Kerf's chip and
 * clear actions, and Home/⌘← caret placement. Register it before `wireSearchFields` so the
 * focus handlers can read a chip's position before Kerf removes or expands it.
 */
export function wireTicketSearchFields(root: HTMLElement, handlers: TicketSearchFieldHandlers): InteractionTeardown {
  const lifetime = createScope();
  lifetime.add(wireTicketSearchSurfaces(root, handlers));
  lifetime.add(
    delegate(
      root,
      'keydown',
      '.ticket-search-field [data-search-field-editor], .ticket-search-form-field [data-search-field-editor]',
      (event, target) => {
        const keyboard = event as KeyboardEvent;
        if (keyboard.key === 'Home' || (keyboard.key === 'ArrowLeft' && (keyboard.metaKey || keyboard.ctrlKey))) {
          event.preventDefault();
          placeSearchFieldCaret(target as HTMLElement, 0);
        }
      },
    ),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SEARCH_ACTIONS.removeToken.selector, (_event, target) => {
      const raw = data(target).tokenValue;
      if (raw)
        withField(root, target, (id) => {
          handlers.removeToken(id, raw);
        });
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SEARCH_ACTIONS.editToken.selector, (_event, target) => {
      const raw = data(target).tokenValue;
      if (raw)
        withField(root, target, (id) => {
          handlers.editToken(id, raw);
        });
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SEARCH_ACTIONS.toggleHelp.selector, (_event, target) => {
      withField(root, target, (id) => {
        handlers.toggleHelp(id);
      });
    }),
  );
  lifetime.add(
    delegate(root, 'mousedown', TICKET_SEARCH_ACTIONS.clear.selector, (event) => {
      event.preventDefault();
    }),
  );
  lifetime.add(
    delegate(root, 'click', TICKET_SEARCH_ACTIONS.clear.selector, (_event, target) => {
      withField(root, target, (id) => {
        handlers.clear(id);
      });
    }),
  );
  return () => {
    lifetime.dispose();
  };
}
