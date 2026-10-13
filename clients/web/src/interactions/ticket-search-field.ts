import { openAnchoredSurface } from '@kerfjs/ui/anchored-surface';
import { placeTokenSearchCaret } from '@kerfjs/ui/token-search-field';
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
  /** Kerf's managed clear emptied the model; close the app's helper surfaces and restore focus. */
  readonly clear: (id: string) => void;
  /** A chip's remove button was pressed (Kerf removes it); restore the caret where the chip sat. */
  readonly removeToken: (id: string, raw: string) => void;
  /** A chip's edit button was pressed (Kerf expands it to text); place the caret after that text. */
  readonly editToken: (id: string, raw: string) => void;
}

/** The element whose descendants belong to one field: its group or form-field wrapper. */
function withField(root: HTMLElement, target: Element, run: (id: string) => void) {
  const id = ticketSearchFieldId(target),
    scope = target.closest<HTMLElement>('.ticket-search-field, .ticket-search-form-field');
  if (id && scope && root.contains(scope)) run(id);
}

/** Read a date helper's Web Awesome inputs (they expose `value` on the host, HS2-HEXGFB) and apply them. */
function applyDate(id: string, surface: Element, handlers: TicketSearchFieldHandlers) {
  const date = surface.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_DATE_INPUT}"]`)?.value;
  if (!date) return;
  const time = surface.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_TIME_INPUT}"]`)?.value ?? '',
    prefix = surface.querySelector<HTMLElement>(TICKET_SEARCH_ACTIONS.applyDate.selector)?.dataset.datePrefix as
      SearchDatePrefix | undefined,
    token = prefix ? dateTokenFromInput(prefix, date, time, navigator.language) : undefined;
  if (prefix && token) handlers.applyDate(id, prefix, `${date}${time ? `T${time}` : ''}`);
}

/**
 * Surfaces hang from the field's end edge, extending over the toolbar rather than cramping their
 * content; on phones they hang from its start edge so a field near the end still leaves a gutter
 * (Kerf clamps to the bare viewport edge).
 */
function surfaceAlign(): 'start' | 'end' {
  return matchMedia('(max-width: 480px)').matches ? 'start' : 'end';
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
 * editor. Their wrappers carry `data-token-search-keep-open`, so pressing inside one does not
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
      align: surfaceAlign(),
      initialFocus: false,
      dismiss: false,
    });
    handle.el.setAttribute('data-token-search-keep-open', '');
    handle.el.addEventListener('click', (event) => {
      if ((event.target as Element).closest(TICKET_SEARCH_ACTIONS.applyDate.selector))
        applyDate(id, handle.el, handlers);
    });
    // Enter inside the date helper applies it instead of submitting an enclosing form.
    handle.el.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' || !(event.target as Element).closest('wa-input')) return;
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
        align: surfaceAlign(),
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
    handle.el.setAttribute('data-token-search-keep-open', '');
    return handle;
  };
  const sync = () => {
    const wanted = new Set<string>();
    for (const marker of root.querySelectorAll<HTMLElement>('[data-ticket-search-surface-state]')) {
      const id = marker.dataset.ticketSearchFor,
        anchor = marker.closest('.ticket-search-field, .ticket-search-form-field');
      if (!id || !anchor) continue;
      const prefix = marker.dataset.datePrefix ?? '',
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
    attributeFilter: ['data-date-prefix', 'data-help-open'],
  });
  sync();
  return () => {
    observer.disconnect();
    for (const key of [...open.keys()]) close(key);
  };
}

/**
 * Wire the delegated actions of every TicketSearchField under `root`. Kerf's
 * `wireTokenSearchFields` owns editor chrome, submit, collapsible behavior, and (with the
 * registered models) parsing, chips, suggestions, edit, removal, and clear; this adds the Hot
 * Sheet helper surfaces (date helper, syntax help), focus restoration around Kerf's chip and
 * clear actions, and Home/⌘← caret placement. Register it before `wireTokenSearchFields` so the
 * focus handlers can read a chip's position before Kerf removes or expands it.
 */
export function wireTicketSearchFields(root: HTMLElement, handlers: TicketSearchFieldHandlers): InteractionTeardown {
  const lifetime = createScope();
  lifetime.add(wireTicketSearchSurfaces(root, handlers));
  lifetime.add(
    delegate(
      root,
      'keydown',
      '.ticket-search-field [data-token-search-editor], .ticket-search-form-field [data-token-search-editor]',
      (event, target) => {
        const keyboard = event as KeyboardEvent;
        if (keyboard.key === 'Home' || (keyboard.key === 'ArrowLeft' && (keyboard.metaKey || keyboard.ctrlKey))) {
          event.preventDefault();
          placeTokenSearchCaret(target as HTMLElement, 0);
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
