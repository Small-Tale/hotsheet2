import './ticket-search-field.css';
import '@kerfjs/ui/token-search-field.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { TokenSearchField, type TokenSearchFieldProps } from '@kerfjs/ui/token-search-field';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { CircleHelp } from 'lucide';

import {
  activeDatePrefix,
  type InlineSearchToken,
  ticketSearchTagSuggestions,
  toTokenSearchToken,
} from '../inline-search';

/**
 * Delegated action names every TicketSearchField renders. Each action element sits inside
 * the field's `.ticket-search-field` group, so a handler resolves the owning field through
 * `ticketSearchFieldId` instead of a per-consumer action name (HS2-N5G6JS).
 */
export const TICKET_SEARCH_ACTIONS = {
  editToken: 'edit-ticket-search-token',
  removeToken: 'remove-ticket-search-token',
  clear: 'clear-ticket-search',
  toggleHelp: 'toggle-ticket-search-help',
  selectTag: 'select-ticket-search-tag',
  applyDate: 'apply-ticket-search-date',
} as const;

/** Native input names inside a field's date helper, resolved relative to that field. */
export const TICKET_SEARCH_DATE_INPUT = 'ticket-search-date';
export const TICKET_SEARCH_TIME_INPUT = 'ticket-search-time';

export interface TicketSearchFieldProps {
  /** Kerf token-search editor id; also the identity delegated handlers receive. */
  id: string;
  label: string;
  query?: string;
  tokens?: readonly InlineSearchToken[];
  /** Forwarded to Kerf: change it when the app replaces the editor text itself. */
  revision?: string | number;
  placeholder?: string;
  disabled?: boolean;
  autofocus?: boolean;
  /** Render as Kerf's managed collapsible toolbar field; `expanded` then reports the open state. */
  collapsible?: boolean;
  expanded?: boolean;
  /** Every tag the project offers; matching suggestions derive from the query's trailing `tag:` text. */
  tags?: readonly string[];
  /** Whether the syntax help popover is open. */
  helpOpen?: boolean;
  /** Offer the syntax help button and popover (default true). */
  help?: boolean;
  clearLabel?: string;
  /**
   * Where the tag, date, and help surfaces render. `floating` (default) hangs them below the
   * group as popovers. `external` renders none inside the group; the consumer places one
   * `TicketSearchSurfaces` for the same `id` in its own stacked layout — for dialogs and other
   * clipping containers where a popover cannot escape.
   */
  surfaces?: 'floating' | 'external';
}

/** Props for the helper surfaces a consumer places itself when the field uses `surfaces="external"`. */
export interface TicketSearchSurfacesProps {
  /** The `id` of the TicketSearchField these surfaces serve. */
  id: string;
  query?: string;
  tokens?: readonly InlineSearchToken[];
  tags?: readonly string[];
  helpOpen?: boolean;
  help?: boolean;
}

const localSearchDateExample = new Intl.DateTimeFormat(undefined, { dateStyle: 'short' }).format(new Date(2026, 8, 1));
const localSearchDateTimeExample = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'short',
  timeStyle: 'short',
}).format(new Date(2026, 8, 1, 11, 5));

/** The owning field id for an element rendered inside a TicketSearchField. */
export function ticketSearchFieldId(target: Element): string | undefined {
  return (
    target.closest<HTMLElement>('[data-ticket-search-for]')?.dataset.ticketSearchFor ??
    target.closest<HTMLElement>('.ticket-search-field')?.querySelector<HTMLElement>('[data-token-search-id]')?.dataset
      .tokenSearchId
  );
}

function TicketSearchHelp() {
  return (
    <aside class="ticket-search-field__help" role="dialog" aria-label="Search syntax" data-token-search-keep-open>
      <header>
        <strong>Search syntax</strong>
        <p>Type words, then add any filters you need.</p>
      </header>
      <dl>
        <div>
          <dt>Tags</dt>
          <dd>
            <code>tag:client</code>
            <code>tag:&quot;needs design&quot;</code>
          </dd>
        </div>
        <div>
          <dt>Content</dt>
          <dd>
            <code>has:attachment</code>
            <code>has:media-annotation</code>
            <code>has:commit</code>
            <code>attachment:*.png</code>
          </dd>
        </div>
        <div>
          <dt>Workflow</dt>
          <dd>
            <code>is:up-next</code>
            <code>is:active</code>
            <code>is:open</code>
            <code>is:closed</code>
            <code>is:duplicate</code>
            <code>is:not-started</code>
            <code>is:started</code>
            <code>is:completed</code>
            <code>is:verified</code>
            <code>is:backlog</code>
            <code>is:archived</code>
          </dd>
        </div>
        <div>
          <dt>Dates</dt>
          <dd>
            <code>updated-after:4h ago</code>
            <code>{`created-after:${localSearchDateExample}`}</code>
            <code>{`completed-before:${localSearchDateTimeExample}`}</code>
            <code>updated-after:2026-09-01T11:05</code>
          </dd>
        </div>
      </dl>
      <div class="ticket-search-field__help-notes">
        <p>
          <strong>Combine filters</strong> with case-insensitive <code>AND</code>, <code>OR</code>, <code>NOT</code>,
          and parentheses.
        </p>
        <code>(client OR server) AND NOT is:archived</code>
        <p>NOT binds before AND, and AND before OR.</p>
        <p>
          <strong>Date fields:</strong> created, completed, started, verified, archived, and updated. Add{' '}
          <code>-before</code> or <code>-after</code>; local, relative, and ISO 8601 dates work.
        </p>
      </div>
    </aside>
  );
}

/**
 * The tag-completion listbox, lifecycle date helper, and syntax-help dialog for one
 * TicketSearchField. Rendered inside the field's group (floating popovers) or, with
 * `surfaces="external"`, placed by the consumer in its own stacked layout; either way the
 * `data-ticket-search-for` attribute tells the shared wiring which field the actions belong to.
 */
export function TicketSearchSurfaces({
  id,
  query = '',
  tokens = [],
  tags = [],
  helpOpen = false,
  help = true,
}: TicketSearchSurfacesProps) {
  const suggestions = ticketSearchTagSuggestions(query, tokens, tags),
    datePrefix = activeDatePrefix(query);
  return (
    <div class="ticket-search-surfaces" data-ticket-search-for={id} data-token-search-keep-open>
      {suggestions.length > 0 ? (
        <div class="ticket-search-field__suggestions" role="listbox" aria-label="Matching tags">
          {suggestions.map((tag) => (
            <button type="button" role="option" data-action={TICKET_SEARCH_ACTIONS.selectTag} data-tag={tag}>
              tag:{tag.includes(' ') ? `"${tag}"` : tag}
            </button>
          ))}
        </div>
      ) : (
        <></>
      )}
      {datePrefix ? (
        <div class="ticket-search-field__date" role="group" aria-label="Date and time helper">
          <label>
            Date
            <input name={TICKET_SEARCH_DATE_INPUT} type="date" />
          </label>
          <label>
            Time (optional)
            <input name={TICKET_SEARCH_TIME_INPUT} type="time" />
          </label>
          <button type="button" data-action={TICKET_SEARCH_ACTIONS.applyDate} data-date-prefix={datePrefix}>
            Apply
          </button>
        </div>
      ) : (
        <></>
      )}
      {help && helpOpen ? <TicketSearchHelp /> : <></>}
    </div>
  );
}

/**
 * The ticket search query editor: Kerf's grouped TokenSearchField plus Hot Sheet's in-place tag
 * completion, lifecycle date helper, and syntax help. Every ticket-search surface (workspace
 * header, workspace-grid rail, saved-view dialog) composes this one component so the helpers
 * cannot drift apart or be forgotten (HS2-N5G6JS). The group's root class is the static
 * `ticket-search-field`; a consumer sizes and places it through its own context selector
 * (for example `.workspace-header__actions > .ticket-search-field`), never by adding a class.
 */
export function TicketSearchField({
  id,
  label,
  query = '',
  tokens = [],
  revision,
  placeholder = 'Search tickets',
  disabled = false,
  autofocus = false,
  collapsible = false,
  expanded = false,
  tags = [],
  helpOpen = false,
  help = true,
  clearLabel = 'Clear search',
  surfaces = 'floating',
}: TicketSearchFieldProps) {
  const open = collapsible ? expanded : true,
    field: Omit<TokenSearchFieldProps, 'collapsible' | 'expanded' | 'expandAction' | 'expandLabel'> = {
      presentation: 'toolbar-group',
      id,
      label,
      query,
      tokens: tokens.map(toTokenSearchToken),
      revision,
      placeholder,
      disabled,
      autofocus,
      editAction: TICKET_SEARCH_ACTIONS.editToken,
      removeAction: TICKET_SEARCH_ACTIONS.removeToken,
      clearAction: TICKET_SEARCH_ACTIONS.clear,
      clearLabel,
      trailing: help ? (
        <button
          type="button"
          class="ticket-search-field__help-button"
          data-action={TICKET_SEARCH_ACTIONS.toggleHelp}
          aria-label="Search syntax help"
          aria-expanded={String(helpOpen)}
          title="Search syntax help"
          data-token-search-keep-open
        >
          <LucideIcon icon={CircleHelp} name="circle-help" />
        </button>
      ) : undefined,
    };
  return (
    <ToolbarControlGroup className="ticket-search-field" expanded={open} single content="search" focusRing="halo">
      {collapsible ? <TokenSearchField {...field} collapsible expanded={expanded} /> : <TokenSearchField {...field} />}
      {open && surfaces === 'floating' ? (
        <TicketSearchSurfaces id={id} query={query} tokens={tokens} tags={tags} helpOpen={helpOpen} help={help} />
      ) : (
        <></>
      )}
    </ToolbarControlGroup>
  );
}
