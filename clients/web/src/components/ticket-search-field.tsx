import './ticket-search-field.css';
import '@kerfjs/ui/search-field.css';

import { Button } from '@kerfjs/ui/button';
import { Chip } from '@kerfjs/ui/chip';
import { flex, rem } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { SearchField } from '@kerfjs/ui/search-field';
import { createSearchFieldModel, type SearchFieldModel } from '@kerfjs/ui/search-field-model';
import { Text } from '@kerfjs/ui/text';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { computed, type ReadonlySignal } from 'kerfjs';
import { action } from 'kerfjs/actions';
import { CircleHelp } from 'lucide';

import { activeDatePrefix } from '../inline-search';

/**
 * Delegated action specs every TicketSearchField renders (markup spreads `.attrs`; handlers
 * register `.selector`; Kerf's SearchField takes the `.value` names). Each action element sits inside
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
  /** A syntax-help example chip: add its token to the field's search (HS2-FR1Y70). */
  insertSyntax: action('insert-ticket-search-syntax'),
} as const;

// Kerf keeps a collapsible field visually open while its model has content. A disabled
// field shows an empty display model so it can close without clearing the real query.
const disabledCollapsedModel = createSearchFieldModel();

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
   * the same id in `wireSearchFields({ models })`.
   */
  model: SearchFieldModel;
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
   * How the toolbar group sizes itself in its Toolbar zone (HS2-8FS5BJ), each a Kerf
   * ToolbarControlGroup policy (HS2-DAMHD1). `inline` (default) keeps Kerf's own collapsed and
   * expanded widths and hides its collapsed icon in tiny toolbars. `grow` is the generic toolbar policy (`sizing="grow"`): the expanded field grows
   * into its row's free room from Kerf's 19rem basis floor and takes the whole row on a compact
   * toolbar, and its collapsed icon leaves the tiniest toolbars (`visibility="hide-collapsed-tiny"`).
   * `row` is the narrow-rail policy (`sizing="fill"`, `placement="end"`): the collapsed icon
   * sits at its stacked row's trailing edge and the expanded field takes a full row
   * of its own, entering from the row above.
   */
  layout?: TicketSearchFieldLayout;
}

/** The toolbar sizing policies a TicketSearchField offers (see `TicketSearchFieldProps.layout`). */
export type TicketSearchFieldLayout = 'inline' | 'grow' | 'row';

const localSearchDateExample = new Intl.DateTimeFormat(undefined, { dateStyle: 'short' }).format(new Date(2026, 8, 1));
const localSearchDateTimeExample = new Intl.DateTimeFormat(undefined, {
  dateStyle: 'short',
  timeStyle: 'short',
}).format(new Date(2026, 8, 1, 11, 5));

/** The owning field id for an element rendered inside a TicketSearchField. */
export function ticketSearchFieldId(target: Element): string | undefined {
  return (
    target.closest<HTMLElement>('[data-ticket-search-for]')?.dataset.ticketSearchFor ??
    target.closest<HTMLElement>('.ticket-search-field')?.querySelector<HTMLElement>('[data-search-field-id]')?.dataset
      .searchFieldId
  );
}

/** The syntax examples, grouped; each example renders as an insertable chip (HS2-65XJ6C, HS2-FR1Y70). */
const SEARCH_SYNTAX_GROUPS: readonly { label: string; examples: readonly string[] }[] = [
  { label: 'Tags', examples: ['tag:client', 'tag:"needs design"'] },
  { label: 'Content', examples: ['has:attachment', 'has:media-annotation', 'has:commit', 'attachment:*.png'] },
  {
    label: 'Workflow',
    examples: [
      'is:up-next',
      'is:active',
      'is:needs-review',
      'is:open',
      'is:closed',
      'is:duplicate',
      'is:not-started',
      'is:started',
      'is:completed',
      'is:verified',
      'is:backlog',
      'is:archived',
    ],
  },
  {
    label: 'Dates',
    examples: [
      'updated-after:4h ago',
      `created-after:${localSearchDateExample}`,
      `completed-before:${localSearchDateTimeExample}`,
      'updated-after:2026-09-01T11:05',
    ],
  },
];

/**
 * One activatable syntax example: Kerf's action Chip (a native button, so Enter and Space work)
 * carrying the example as its item id; choosing it adds the example to the search (HS2-FR1Y70).
 */
function SyntaxChip({ example }: { example: string }) {
  return (
    <Chip
      action={TICKET_SEARCH_ACTIONS.insertSyntax.attrs['data-action']}
      actionLabel={`Add ${example} to the search`}
      itemId={example}
      size="compact"
      // Its label reads like the token chip it inserts, and truncates rather than overflowing a
      // phone-width popover; the action label keeps the whole example.
      truncate
    >
      {example}
    </Chip>
  );
}

/**
 * The syntax-help content. Kerf `openAnchoredSurface` hosts it (the non-modal `Search syntax`
 * dialog, its chrome, position, and top-layer or `wa-dialog` hosting) while the field's
 * `helpOpen` state is set (HS2-8TE69P).
 */
export function TicketSearchHelp({ id }: { id: string }) {
  return (
    <div
      class="ticket-search-surfaces ticket-search-field__help"
      data-ticket-search-for={id}
      data-search-field-keep-open
    >
      <List gap="xs">
        <List gap="2xs">
          <strong>Search syntax</strong>
          <Text tone="quiet" size="compact" flush>
            Type words, then add any filters you need.
          </Text>
        </List>
        {/* Each group is a label column beside wrapping example chips; in a narrow popover the
            examples drop under the label instead of clipping. */}
        <List gap="xs">
          {SEARCH_SYNTAX_GROUPS.map((group) => (
            <Row gap="xs" vAlign="top" wrap>
              <Row flex={flex(0, 0, rem(4.5))}>
                <Text variant="span" tone="quiet" size="compact" data-syntax-group={group.label}>
                  <strong>{group.label}</strong>
                </Text>
              </Row>
              <Row gap="2xs" wrap flex={flex(1, 1, rem(12.5))}>
                {group.examples.map((example) => (
                  <SyntaxChip example={example} />
                ))}
              </Row>
            </Row>
          ))}
        </List>
        <List gap="2xs" dividerSides="t">
          <Text tone="quiet" size="compact" flush>
            <strong>Combine filters</strong> with case-insensitive <code>AND</code>, <code>OR</code>, <code>NOT</code>,
            and parentheses.
          </Text>
          <Row>
            <SyntaxChip example="(client OR server) AND NOT is:archived" />
          </Row>
          <Text tone="quiet" size="compact" flush>
            NOT binds before AND, and AND before OR.
          </Text>
          <Text tone="quiet" size="compact" flush>
            <strong>Date fields:</strong> created, completed, started, verified, archived, and updated. Add{' '}
            <code>-before</code> or <code>-after</code>; local, relative, and ISO 8601 dates work.
          </Text>
        </List>
      </List>
    </div>
  );
}

/**
 * The lifecycle date helper's content for one field while a trailing lifecycle filter such as
 * `updated-after:` is typed. Kerf `openAnchoredSurface` hosts it; `data-ticket-search-for` tells
 * the shared wiring which field its Apply belongs to.
 */
export function TicketSearchDateHelper({ id, prefix }: { id: string; prefix: string }) {
  return (
    <div
      class="ticket-search-surfaces ticket-search-field__date"
      role="group"
      aria-label="Date and time helper"
      data-ticket-search-for={id}
      data-search-field-keep-open
    >
      <label class="ticket-search-field__date-field">
        <span>Date</span>
        <input name={TICKET_SEARCH_DATE_INPUT} type="date" />
      </label>
      <label class="ticket-search-field__date-field">
        <span>Time (Optional)</span>
        <input name={TICKET_SEARCH_TIME_INPUT} type="time" />
      </label>
      <div class="ticket-search-field__date-apply">
        <Button
          size="compact"
          action={TICKET_SEARCH_ACTIONS.applyDate.value}
          rootAttributes={{ 'data-date-prefix': prefix }}
        >
          Apply
        </Button>
      </div>
    </div>
  );
}

/**
 * The declarative state of one field's floating surfaces, read by `wireTicketSearchFields`, which
 * opens and closes the Kerf anchored surfaces to match (HS2-8TE69P). An empty `data-date-prefix`,
 * or `data-surfaces-open="false"`, means no date helper.
 */
function TicketSearchSurfaceState({
  id,
  model,
  open,
  helpOpen,
}: {
  id: string;
  model: SearchFieldModel;
  open: boolean;
  helpOpen: boolean;
}) {
  return (
    <span
      hidden
      data-ticket-search-surface-state
      data-ticket-search-for={id}
      data-surfaces-open={String(open)}
      data-date-prefix={typedDatePrefix(model)}
      data-help-open={String(open && helpOpen)}
    />
  );
}

const typedDatePrefixes = new WeakMap<SearchFieldModel, ReadonlySignal<string>>();

/**
 * The lifecycle date filter being typed, bound straight onto the surface marker's attribute: the
 * field's render never reads the per-keystroke model state, so typing does not re-render the host
 * (HS2-X72PD6). One stable `computed` per model keeps the binding from going stale.
 */
function typedDatePrefix(model: SearchFieldModel): ReadonlySignal<string> {
  let prefix = typedDatePrefixes.get(model);
  if (!prefix) {
    prefix = computed(() => activeDatePrefix(model.state.value.query) ?? '');
    typedDatePrefixes.set(model, prefix);
  }
  return prefix;
}

/**
 * The syntax-help disclosure as Kerf's compact trailing action (KF-R056AJ, HS2-P1689S): Kerf sizes,
 * shapes, and states it like the clear button; `data-search-field-keep-open` keeps the field's
 * surfaces open while it is pressed.
 */
function ticketSearchHelpTrailing(help: boolean, open: boolean) {
  return help
    ? {
        trailingAction: {
          icon: <LucideIcon icon={CircleHelp} name="circle-help" />,
          label: 'Search Syntax Help',
          action: TICKET_SEARCH_ACTIONS.toggleHelp.value,
          expanded: open,
          haspopup: 'dialog' as const,
          attributes: { 'data-search-field-keep-open': '' },
        },
      }
    : {};
}

export interface TicketSearchFormFieldProps {
  /** Kerf token-search editor id; also the identity delegated handlers receive. */
  id: string;
  /** Visible form label (Kerf renders it with the same inset and typography as TextField). */
  label: string;
  /** The Kerf-managed search model, registered under `id` in `wireSearchFields({ models })`. */
  model: SearchFieldModel;
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
 * Kerf's `SearchField presentation="form-field"` (visible label, hint, required marker,
 * full width, no Toolbar) driven by the same managed model, with the syntax-help button. Its
 * date/help surfaces are Kerf anchored surfaces, which stay interactive inside a `wa-dialog`
 * (KF-F52T5D). The app-owned wrapper carries the field id
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
    // A Kerf List stacks the field and its surface state (HS2-4GX2YS).
    <List gap="2xs" className="ticket-search-form-field" rootAttributes={{ 'data-ticket-search-for': id }}>
      <SearchField
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
        {...ticketSearchHelpTrailing(help, helpOpen)}
      />
      <TicketSearchSurfaceState id={id} model={model} open={!disabled} helpOpen={help && helpOpen} />
    </List>
  );
}

/**
 * The ticket search query editor: Kerf's grouped SearchField driven by a managed
 * `SearchFieldModel` (grammar, chips, in-place tag completion), plus Hot Sheet's lifecycle date
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
  layout = 'inline',
}: TicketSearchFieldProps) {
  const open = collapsible ? expanded && !disabled : true,
    // Inferred, not annotated: `Omit` over Kerf's union props (beta.64 `trailing` XOR `trailingAction`)
    // would collapse the branches.
    field = {
      presentation: 'toolbar-group' as const,
      id,
      label,
      model: disabled && collapsible ? disabledCollapsedModel : model,
      placeholder,
      disabled,
      autofocus,
      editAction: TICKET_SEARCH_ACTIONS.editToken.value,
      removeAction: TICKET_SEARCH_ACTIONS.removeToken.value,
      clearAction: TICKET_SEARCH_ACTIONS.clear.value,
      clearLabel,
      ...ticketSearchHelpTrailing(help, helpOpen),
      // The field's token colors are set on this app-owned hook, shared with TicketSearchFormField
      // (HS2-8FS5BJ, HS2-RXHZVR), never by reaching into Kerf's `.kui-search-field`.
      className: 'ticket-search-field__query',
      fill: layout === 'row',
    },
    content = (
      <>
        {collapsible ? <SearchField {...field} collapsible expanded={open} /> : <SearchField {...field} />}
        <TicketSearchSurfaceState id={id} model={model} open={open} helpOpen={help && helpOpen} />
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
      visibility="hide-collapsed-tiny"
    >
      {content}
    </ToolbarControlGroup>
  );
}
