import { List } from '@kerfjs/ui/list';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';

/** Package-owned list geometry with passive content and an explicitly bounded scroll example. */
export function ListDemo() {
  const rows = (count: number) =>
    Array.from({ length: count }, (_, index) => <ListInsetText>{`Row ${index + 1}`}</ListInsetText>);
  return (
    <section aria-label="List layout variants">
      <h2>No gap</h2>
      <List>{rows(3)}</List>
      <h2>Standard gap</h2>
      <List gap>{rows(3)}</List>
      <h2>Custom gap and scrolling</h2>
      <div style="display:flex;height:12rem" role="region" aria-label="Scrollable list example">
        <List gap="var(--kui-space-s)" flex="1 1 0%" scrollable dividerSides="trbl">
          {rows(12)}
        </List>
      </div>
    </section>
  );
}
