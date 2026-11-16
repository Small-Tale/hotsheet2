import './ticket-reference-picker.css';

import { Button } from '@kerfjs/ui/button';
import { rem, uiColor } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { ListItem } from '@kerfjs/ui/list-item';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { CheckCircle2, Search } from 'lucide';

import { type DuplicateTarget, duplicateTargetKey } from '../ticket-close';

export interface TicketReferencePickerProps {
  /** The search field's form name; the owning interaction group listens on it. */
  fieldName: string;
  query: string;
  /** Matches to offer, already excluding the ticket the dialog acts on. */
  candidates: readonly DuplicateTarget[];
  selected?: DuplicateTarget;
  searching?: boolean;
  /** `data-action` of a result row; the row's `data-item-id` is its {@link duplicateTargetKey}. */
  selectAction: string;
  /** `data-action` of the selected ticket's Change button. */
  clearAction: string;
  /** The relationship icon every result row shows. */
  candidateIcon: SafeHtml;
}

/**
 * Search every open project for one existing ticket and show the choice (HS2-MCGCPQ). The close
 * dialog's duplicate target and the Inspector's follow-up link share this picker so the same
 * lookup looks and behaves the same everywhere. A result shows its number, its project, and its
 * title. A choice replaces the results with a success banner and a Change button.
 */
export function TicketReferencePicker({
  fieldName,
  query,
  candidates,
  selected,
  searching = false,
  selectAction,
  clearAction,
  candidateIcon,
}: TicketReferencePickerProps) {
  return (
    <div class="ticket-reference-picker" data-component="ticket-reference-picker">
      <List gap="xs">
        <TextField
          name={fieldName}
          label="Existing Ticket"
          placeholder="Search by ticket number or title"
          value={query}
          icon={<LucideIcon icon={Search} name="search" size={16} color={uiColor('neutral-on-quiet')} />}
        />
        {selected && (
          <StateBanner
            tone="success"
            icon={<LucideIcon icon={CheckCircle2} name="check-circle-2" />}
            title={selected.slug}
            badge={selected.projectName}
            detail={selected.title}
            copyLayout="stacked"
            action={
              <Button type="button" emphasis="plain" size="compact" action={clearAction}>
                Change
              </Button>
            }
          />
        )}
        {!selected && Boolean(query.trim()) && (
          // Kerf's framed, bounded result picker (KF-SHW2J9, HS2-Z46VA7) owns the border,
          // radius, focus-ring inset, and min/max scroll bounds.
          <List
            scrollable
            frame
            minHeight={rem(4.25)}
            maxHeight={rem(14)}
            className="ticket-reference-picker__results"
            rootAttributes={{
              'aria-label': 'Matching tickets',
              'aria-busy': String(searching),
            }}
          >
            {candidates.map((candidate) => (
              <ListItem
                action={selectAction}
                itemId={duplicateTargetKey(candidate)}
                icon={candidateIcon}
                label={
                  <>
                    {candidate.slug}{' '}
                    <Text variant="span" size="compact" tone="quiet">
                      {candidate.projectName}
                    </Text>
                  </>
                }
                description={
                  <Text variant="span" wrap="truncate">
                    {candidate.title}
                  </Text>
                }
                multiline
              />
            ))}
            {!searching && candidates.length === 0 && <Text tone="quiet">No matching tickets.</Text>}
          </List>
        )}
      </List>
    </div>
  );
}
