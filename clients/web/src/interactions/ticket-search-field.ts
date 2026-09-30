import { placeTokenSearchCaret } from '@kerfjs/ui/token-search-field';
import { delegate, delegateCapture } from 'kerfjs';

import {
  TICKET_SEARCH_ACTIONS,
  TICKET_SEARCH_DATE_INPUT,
  TICKET_SEARCH_TIME_INPUT,
  ticketSearchFieldId,
} from '../components/ticket-search-field';
import { dateTokenFromInput, type InlineSearchToken, type SearchDatePrefix } from '../inline-search';
import { data } from './dom';

/**
 * Per-field callbacks for the actions every TicketSearchField renders. Each receives the owning
 * field id so one registration serves the workspace search, the workspace-grid rail, the
 * saved-view dialog, and any later ticket-search surface (HS2-N5G6JS).
 */
export interface TicketSearchFieldHandlers {
  /** A tag suggestion was chosen; replace the trailing `tag:` text with its chip. */
  readonly selectTag: (id: string, tag: string) => void;
  /** The date helper produced a chip for the trailing lifecycle filter. */
  readonly applyDate: (id: string, prefix: SearchDatePrefix, token: InlineSearchToken) => void;
  readonly toggleHelp: (id: string) => void;
  /** Kerf's managed clear; the editor is already empty when this runs. */
  readonly clear: (id: string, editor: HTMLElement | undefined) => void;
  readonly removeToken: (id: string, raw: string) => void;
  /** A chip's edit action, click-count-2 click, or double-click. */
  readonly editToken: (id: string, event: Event, target: Element) => void;
}

/** The element whose descendants belong to one field: its surfaces block or its group. */
function withField(root: HTMLElement, target: Element, run: (id: string, scope: HTMLElement) => void) {
  const id = ticketSearchFieldId(target),
    scope = target.closest<HTMLElement>('.ticket-search-surfaces, .ticket-search-field');
  if (id && scope && root.contains(scope)) run(id, scope);
}

function fieldEditor(root: HTMLElement, id: string): HTMLElement | undefined {
  for (const editor of root.querySelectorAll<HTMLElement>('[data-token-search-editor]'))
    if (editor.dataset.tokenSearchEditor === id) return editor;
  return undefined;
}

function applyDate(root: HTMLElement, target: Element, handlers: TicketSearchFieldHandlers) {
  withField(root, target, (id, scope) => {
    const date = scope.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_DATE_INPUT}"]`)?.value;
    if (!date) return;
    const time = scope.querySelector<HTMLInputElement>(`[name="${TICKET_SEARCH_TIME_INPUT}"]`)?.value ?? '',
      prefix = scope.querySelector<HTMLElement>(`[data-action="${TICKET_SEARCH_ACTIONS.applyDate}"]`)?.dataset
        .datePrefix as SearchDatePrefix | undefined,
      token = prefix ? dateTokenFromInput(prefix, date, time, navigator.language) : undefined;
    if (prefix && token) handlers.applyDate(id, prefix, token);
  });
}

/**
 * Wire the delegated actions of every TicketSearchField under `root`. Kerf's
 * `wireTokenSearchFields` still owns editor chrome, submit, and collapsible behavior; this
 * adds the Hot Sheet helper surfaces (tag completion, date helper, syntax help), chip edit
 * and removal, managed clear, and Home/⌘← caret placement.
 */
export function wireTicketSearchFields(root: HTMLElement, handlers: TicketSearchFieldHandlers): void {
  const action = (name: string) => `[data-action="${name}"]`;
  delegate(root, 'keydown', '.ticket-search-field [data-token-search-editor]', (event, target) => {
    const keyboard = event as KeyboardEvent;
    if (keyboard.key === 'Home' || (keyboard.key === 'ArrowLeft' && (keyboard.metaKey || keyboard.ctrlKey))) {
      event.preventDefault();
      placeTokenSearchCaret(target as HTMLElement, 0);
    }
  });
  // Choosing a suggestion must not move focus out of the editor before the chip lands.
  delegate(root, 'mousedown', action(TICKET_SEARCH_ACTIONS.selectTag), (event) => {
    event.preventDefault();
  });
  delegateCapture(root, 'pointerdown', action(TICKET_SEARCH_ACTIONS.selectTag), (event) => {
    event.preventDefault();
  });
  delegate(root, 'click', action(TICKET_SEARCH_ACTIONS.selectTag), (_event, target) => {
    withField(root, target, (id) => {
      handlers.selectTag(id, data(target).tag!);
    });
  });
  delegate(root, 'click', action(TICKET_SEARCH_ACTIONS.removeToken), (_event, target) => {
    const raw = data(target).tokenValue;
    if (raw)
      withField(root, target, (id) => {
        handlers.removeToken(id, raw);
      });
  });
  delegate(root, 'click', action(TICKET_SEARCH_ACTIONS.editToken), (event, target) => {
    withField(root, target, (id) => {
      handlers.editToken(id, event, target);
    });
  });
  delegate(root, 'click', '.ticket-search-field [data-component="token-search-token"]', (event, target) => {
    if ((event as MouseEvent).detail === 2)
      withField(root, target, (id) => {
        handlers.editToken(id, event, target);
      });
  });
  delegate(root, 'dblclick', '.ticket-search-field [data-component="token-search-token"]', (event, target) => {
    withField(root, target, (id) => {
      handlers.editToken(id, event, target);
    });
  });
  delegate(root, 'click', action(TICKET_SEARCH_ACTIONS.toggleHelp), (_event, target) => {
    withField(root, target, (id) => {
      handlers.toggleHelp(id);
    });
  });
  delegate(root, 'click', action(TICKET_SEARCH_ACTIONS.applyDate), (_event, target) => {
    applyDate(root, target, handlers);
  });
  // Enter inside the date helper applies it instead of submitting an enclosing form.
  delegate(root, 'keydown', '.ticket-search-field__date input', (event, target) => {
    if ((event as KeyboardEvent).key !== 'Enter') return;
    event.preventDefault();
    applyDate(root, target, handlers);
  });
  delegate(root, 'mousedown', action(TICKET_SEARCH_ACTIONS.clear), (event) => {
    event.preventDefault();
  });
  delegate(root, 'click', action(TICKET_SEARCH_ACTIONS.clear), (_event, target) => {
    withField(root, target, (id) => {
      const editor = fieldEditor(root, id);
      if (editor) editor.textContent = '';
      handlers.clear(id, editor);
    });
  });
}
