import './ticket-search-field.css';
import '@kerfjs/ui/token-search-field.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { TokenSearchField } from '@kerfjs/ui/token-search-field';
import type { TokenSearchModel } from '@kerfjs/ui/token-search-model';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { action } from 'kerfjs/actions';
import { CircleHelp } from 'lucide';

import { activeDatePrefix } from '../inline-search';

/**
 * Delegated action specs every TicketSearchField renders (markup spreads `.attrs`; handlers
 * register `.selector`; Kerf's TokenSearchField takes the `.value` names). Each action element sits inside
 * the field's `.ticket-search-field` group, so a handler resolves the owning field through
 * `ticketSearchFieldId` instead of a per-consumer action name (HS2-N5G6JS). Chip edit, chip
 * removal, and clear are Kerf's managed model actions (HS2-5JXBQY); the app's handlers for
 * those names only restore focus and close its own helper surfaces.
 */
export const TICKET_SEARCH_ACTIONS = {
  editToken: action('edit-ticket-search-token'),
  removeToken: action('remove-ticket-search-token'),
  clear: action('clear-ticket-search'),
  toggleHelp: action('toggle-ticket-search-help'),
  applyDate: action('apply-ticket-search-date'),
} as const;

/** Native input names inside a field's date helper, resolved relative to that field. */
export const TICKET_SEARCH_DATE_INPUT = 'ticket-search-date';
export const TICKET_SEARCH_TIME_INPUT = 'ticket-search-time';

export interface TicketSearchFieldProps {
  /** Kerf token-search editor id; also the identity delegated handlers receive. */
  id: string;
  label: string;
  /**
   * The Kerf-managed search model (`createTicketSearchModel`): it owns the query text, the
   * committed chips, the `tag:` suggestions, and the edit/remove/clear actions; register it under
   * the same id in `wireTokenSearchFields({ models })`.
   */
  model: TokenSearchModel;
  placeholder?: string;
  disabled?: boolean;
  autofocus?: boolean;
  /** Render as Kerf's managed collapsible toolbar field; `expanded` then reports the open state. */
  collapsible?: boolean;
  expanded?: boolean;
  /** Whether the syntax help popover is open. */
  helpOpen?: boolean;
  /** Offer the syntax help button and popover (default true). */
  help?: boolean;
  clearLabel?: string;
  /**
   * Where the date and help surfaces render. `floating` (default) hangs them below the group as
   * popovers. `external` renders none inside the group; the consumer places one
   * `TicketSearchSurfaces` for the same `id` in its own stacked layout — for dialogs and other
   * clipping containers where a popover cannot escape. Kerf's tag suggestions always render in
   * flow inside the field itself.
   */
  surfaces?: 'floating' | 'external';
  /**
   * How the toolbar group sizes itself in its Toolbar zone (HS2-8FS5BJ), each a Kerf
   * ToolbarControlGroup policy (HS2-DAMHD1). `inline` (default) keeps Kerf's own collapsed and
   * expanded widths. `grow` is the workspace header policy (`sizing="grow"`): the expanded field grows
   * into its row's free room from Kerf's 19rem basis floor and takes the whole row on a compact
   * toolbar, and its collapsed icon leaves the tiniest toolbars (`visibility="hide-collapsed-tiny"`). `row` is the narrow-rail policy (`sizing="fill"`, `placement="end"`): the
   * collapsed icon sits at its stacked row's trailing edge and the expanded field takes a full row
   * of its own, entering from the row above.
   */
  layout?: TicketSearchFieldLayout;
}

/** The toolbar sizing policies a TicketSearchField offers (see `TicketSearchFieldProps.layout`). */
export type TicketSearchFieldLayout = 'inline' | 'grow' | 'row';

/** Props for the helper surfaces a consumer places itself when the field uses `surfaces="external"`. */
export interface TicketSearchSurfacesProps {
  /** The `id` of the TicketSearchField these surfaces serve. */
  id: string;
  model: TokenSearchModel;
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
 * The lifecycle date helper and syntax-help dialog for one TicketSearchField (its tag completion
 * is Kerf's, rendered by the model inside the field). Rendered inside the field's group (floating
 * popovers) or, with `surfaces="external"`, placed by the consumer in its own stacked layout;
 * either way the `data-ticket-search-for` attribute tells the shared wiring which field the
 * actions belong to.
 */
export function TicketSearchSurfaces({ id, model, helpOpen = false, help = true }: TicketSearchSurfacesProps) {
  const datePrefix = activeDatePrefix(model.state.value.query);
  return (
    <div class="ticket-search-surfaces" data-ticket-search-for={id} data-token-search-keep-open>
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
          <button type="button" {...TICKET_SEARCH_ACTIONS.applyDate.attrs} data-date-prefix={datePrefix}>
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

function TicketSearchHelpButton({ open }: { open: boolean }) {
  return (
    <button
      type="button"
      class="ticket-search-field__help-button"
      {...TICKET_SEARCH_ACTIONS.toggleHelp.attrs}
      aria-label="Search syntax help"
      aria-expanded={String(open)}
      title="Search syntax help"
      data-token-search-keep-open
    >
      <LucideIcon icon={CircleHelp} name="circle-help" />
    </button>
  );
}

export interface TicketSearchFormFieldProps {
  /** Kerf token-search editor id; also the identity delegated handlers receive. */
  id: string;
  /** Visible form label (Kerf renders it with the same inset and typography as `wa-input`). */
  label: string;
  /** The Kerf-managed search model, registered under `id` in `wireTokenSearchFields({ models })`. */
  model: TokenSearchModel;
  placeholder?: string;
  disabled?: boolean;
  /** Supporting text below the field, referenced by `aria-describedby`. */
  hint?: string;
  /** Marks the field required (`aria-required` plus Kerf's marker); the app validates the value. */
  required?: boolean;
  helpOpen?: boolean;
  help?: boolean;
  clearLabel?: string;
}

/**
 * The ticket search query editor as a form field beside Web Awesome controls (HS2-E40KC0):
 * Kerf's `TokenSearchField presentation="form-field"` (visible label, hint, required marker,
 * full width, no Toolbar) driven by the same managed model, with the syntax-help button and
 * the date/help surfaces stacked below it in flow. The app-owned wrapper carries the field id
 * for the shared delegated wiring. Its chips share the grouped field's quieter tint through the same
 * `ticket-search-field__query` hook Kerf renders on the form-field root (KF-5G8WJ0, HS2-RXHZVR).
 */
export function TicketSearchFormField({
  id,
  label,
  model,
  placeholder = 'Search tickets',
  disabled = false,
  hint,
  required = false,
  helpOpen = false,
  help = true,
  clearLabel = 'Clear search',
}: TicketSearchFormFieldProps) {
  return (
    <div class="ticket-search-form-field" data-ticket-search-for={id}>
      <TokenSearchField
        presentation="form-field"
        className="ticket-search-field__query"
        id={id}
        label={label}
        model={model}
        placeholder={placeholder}
        disabled={disabled}
        hint={hint}
        required={required}
        editAction={TICKET_SEARCH_ACTIONS.editToken.value}
        removeAction={TICKET_SEARCH_ACTIONS.removeToken.value}
        clearAction={TICKET_SEARCH_ACTIONS.clear.value}
        clearLabel={clearLabel}
        trailing={help ? <TicketSearchHelpButton open={helpOpen} /> : undefined}
      />
      <TicketSearchSurfaces id={id} model={model} helpOpen={helpOpen} help={help} />
    </div>
  );
}

/**
 * The ticket search query editor: Kerf's grouped TokenSearchField driven by a managed
 * `TokenSearchModel` (grammar, chips, in-place tag completion), plus Hot Sheet's lifecycle date
 * helper and syntax help. Every ticket-search surface (workspace header, workspace-grid rail,
 * saved-view dialog) composes this one component so the helpers cannot drift apart or be
 * forgotten (HS2-N5G6JS, HS2-5JXBQY). A consumer chooses how the group sizes in its toolbar through
 * `layout`, never by styling the `ticket-search-field` root from its own stylesheet (HS2-8FS5BJ).
 */
export function TicketSearchField({
  id,
  label,
  model,
  placeholder = 'Search tickets',
  disabled = false,
  autofocus = false,
  collapsible = false,
  expanded = false,
  helpOpen = false,
  help = true,
  clearLabel = 'Clear search',
  surfaces = 'floating',
  layout = 'inline',
}: TicketSearchFieldProps) {
  const open = collapsible ? expanded : true,
    // Inferred, not annotated: `Omit` over Kerf's union props (beta.64 `trailing` XOR `trailingAction`)
    // would collapse the branches.
    field = {
      presentation: 'toolbar-group' as const,
      id,
      label,
      model,
      placeholder,
      disabled,
      autofocus,
      editAction: TICKET_SEARCH_ACTIONS.editToken.value,
      removeAction: TICKET_SEARCH_ACTIONS.removeToken.value,
      clearAction: TICKET_SEARCH_ACTIONS.clear.value,
      clearLabel,
      trailing: help ? <TicketSearchHelpButton open={helpOpen} /> : undefined,
      // The field's token colors are set on this app-owned hook, shared with TicketSearchFormField
      // (HS2-8FS5BJ, HS2-RXHZVR), never by reaching into Kerf's `.kui-token-search`.
      className: 'ticket-search-field__query',
      fill: layout === 'row',
    },
    content = (
      <>
        {collapsible ? (
          <TokenSearchField {...field} collapsible expanded={expanded} />
        ) : (
          <TokenSearchField {...field} />
        )}
        {open && surfaces === 'floating' ? (
          <TicketSearchSurfaces id={id} model={model} helpOpen={helpOpen} help={help} />
        ) : (
          <></>
        )}
      </>
    );
  // Each layout is a Kerf ToolbarControlGroup policy (HS2-DAMHD1). `grow` (Kerf `sizing="grow"`,
  // HS2-AEK8GK) grows the open field from Kerf's 19rem basis floor, which holds in a content-sized
  // trailing zone since KF-K4VBTS, and drops the collapsed icon from the tiniest toolbars
  // (`visibility="hide-collapsed-tiny"`). `row` fills a wrapping row (with Kerf's stacked-row
  // entrance) and sits at its row's trailing edge while collapsed.
  if (layout === 'grow')
    return (
      <ToolbarControlGroup
        className="ticket-search-field"
        expanded={open}
        expandedOverflow="visible"
        single
        content="search"
        focusRing="halo"
        sizing="grow"
        visibility="hide-collapsed-tiny"
      >
        {content}
      </ToolbarControlGroup>
    );
  if (layout === 'row')
    return (
      <ToolbarControlGroup
        className="ticket-search-field"
        expanded={open}
        expandedOverflow="visible"
        single
        content="search"
        focusRing="halo"
        sizing="fill"
        placement="end"
      >
        {content}
      </ToolbarControlGroup>
    );
  return (
    <ToolbarControlGroup
      className="ticket-search-field"
      expanded={open}
      expandedOverflow="visible"
      single
      content="search"
      focusRing="halo"
    >
      {content}
    </ToolbarControlGroup>
  );
}
