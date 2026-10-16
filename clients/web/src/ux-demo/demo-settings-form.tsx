import { ContentItem } from '@kerfjs/ui/content-item';
import { List, type ListProps } from '@kerfjs/ui/list';
import { Text } from '@kerfjs/ui/text';

/**
 * A catalog entry's settings form (HS2-3AZ08H). Kerf's Catalog settings pane leaves its content
 * inset to the app, so a Kerf List stacks the controls with the standard gap and 8px control insets
 * instead of demo stylesheet rules. `settings` names the form for the entry's delegated `change`
 * wiring (`[data-settings="…"] [name]`).
 */
export function DemoSettingsForm({ settings, children }: { settings: string; children: ListProps['children'] }) {
  return (
    <form data-settings={settings}>
      <List gap="m" controlInsets="rbl">
        {children}
      </List>
    </form>
  );
}

/**
 * The Project Settings workspace stand-in that the shell and workspace demos show in Settings mode
 * (HS2-3AZ08H): a framed Kerf ContentItem surface at the reading inset with a Text title, description,
 * and optional controls, instead of a demo-styled card.
 */
export function DemoSettingsPreview({
  description,
  children,
}: {
  description: string;
  children?: ListProps['children'];
}) {
  return (
    <section aria-label="Project Settings">
      <List inset="document">
        <ContentItem appearance="surface" frame="framed">
          <List gap="xs">
            <Text variant="h2">Project Settings</Text>
            <Text variant="p" tone="quiet">
              {description}
            </Text>
            {children}
          </List>
        </ContentItem>
      </List>
    </section>
  );
}
