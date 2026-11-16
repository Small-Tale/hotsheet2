import type { KerfUiContent } from '@kerfjs/ui';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { Text } from '@kerfjs/ui/text';

export interface OrchestrationInspectorSectionProps {
  /** The section's title, also its accessible name and `data-orchestration-section` value. */
  label: string;
  /** A quiet line under the section's rows, such as what an action will ask first. */
  note?: string;
  children?: KerfUiContent;
}

/**
 * HS2-J89JC7, HS2-2XJSJK: one titled section of the Orchestration inspector (Kerf `ListHeader` over its
 * rows), shared by the card sections and the overview so every section has the same header and insets.
 */
export function OrchestrationInspectorSection({ label, note, children }: OrchestrationInspectorSectionProps) {
  return (
    <section aria-label={label} data-orchestration-section={label}>
      <List gap="xs">
        <ListHeader label={label} />
        {children}
        {note ? (
          <List textInsets="rl">
            <Text tone="quiet" size="compact" flush>
              {note}
            </Text>
          </List>
        ) : undefined}
      </List>
    </section>
  );
}
