import { placeTokenSearchCaret } from '@kerfjs/ui/token-search-field';
import { delegate } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  TICKET_SEARCH_ACTIONS,
  TICKET_SEARCH_DATE_INPUT,
  TICKET_SEARCH_TIME_INPUT,
  ticketSearchFieldId,
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

/** The element whose descendants belong to one field: its surfaces block or its group. */
function withField(root: HTMLElement, target: Element, run: (id: string, scope: HTMLElement) => void) {
  const id = ticketSearchFieldId(target),
    scope = target.closest<HTMLElement>('.ticket-search-surfaces, .ticket-search-field, .ticket-search-form-field');
  if (id && scope && root.contains(scope)) run(id, scope);
}

function applyDate(root: HTMLElement, target: Element, handlers: TicketSearchFieldHandlers) {
  withField(root, target, (id, scope) => {
    const date = scope.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_DATE_INPUT}"]`)?.value;
    if (!date) return;
    const time = scope.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_TIME_INPUT}"]`)?.value ?? '',
      prefix = scope.querySelector<HTMLElement>(TICKET_SEARCH_ACTIONS.applyDate.selector)?.dataset.datePrefix as
        SearchDatePrefix | undefined,
      token = prefix ? dateTokenFromInput(prefix, date, time, navigator.language) : undefined;
    if (prefix && token) handlers.applyDate(id, prefix, `${date}${time ? `T${time}` : ''}`);
  });
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
    delegate(root, 'click', TICKET_SEARCH_ACTIONS.applyDate.selector, (_event, target) => {
      applyDate(root, target, handlers);
    }),
  );
  // Enter inside the date helper applies it instead of submitting an enclosing form.
  lifetime.add(
    delegate(root, 'keydown', '.ticket-search-field__date input', (event, target) => {
      if ((event as KeyboardEvent).key !== 'Enter') return;
      event.preventDefault();
      applyDate(root, target, handlers);
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
