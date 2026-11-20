import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Switch } from '@kerfjs/ui/switch';
import { Text } from '@kerfjs/ui/text';

import {
  FEATURE_AREAS,
  FEATURE_GROUP_TITLES,
  type FeatureGroup,
  featureOn,
  type FeatureSettingsView,
} from '../feature-areas';
import { SHELL_AND_GLOBAL_ACTIONS } from '../interaction-attrs/shell-and-global';

export interface FeatureSettingsProps {
  /** The machine-wide choices; unread, every feature shows its registered default. */
  settings?: FeatureSettingsView;
  /** Why the last read or save failed. */
  message?: string;
}

/** The core areas that are never listed, named once at the foot of the page. */
export const ALWAYS_ON_AREAS =
  'Always on: tickets, search, notes, notifications, permissions, the terminal drawer, ticket sources, and settings.';

/**
 * App Settings › Features (HS2-3GJZGG, docs/06-clients.md §6.12): one Kerf Switch per feature area,
 * saved machine-wide the moment it changes (the maintainer chose Switch for settings that apply at
 * once). Each switch's List takes the 8px control inset so its track lines up with the headings' text.
 */
export function FeatureSettings({ settings, message = '' }: FeatureSettingsProps) {
  const rows = (group: FeatureGroup) => (
    <List gap="xs" controlInsets="l">
      {FEATURE_AREAS.filter((area) => area.group === group).map((area) => (
        <Switch
          action={SHELL_AND_GLOBAL_ACTIONS.setFeature.value}
          name={`feature-${area.id}`}
          value={area.id}
          checked={featureOn(area.id, settings)}
          label={area.label}
          hint={area.hint}
          rootAttributes={{ 'data-feature-row': area.id }}
        />
      ))}
    </List>
  );
  const subsection = (group: Exclude<FeatureGroup, 'experimental'>) => (
    <List gap="xs">
      <Text variant="h3">{FEATURE_GROUP_TITLES[group]}</Text>
      {rows(group)}
    </List>
  );
  return (
    <List gap="l">
      <Text tone="quiet">
        Choose which parts of Hot Sheet appear on every device that uses this computer’s Hot Sheet. A feature that is
        off has no buttons, menu items, or shortcuts. Its data is kept, so turning it back on restores it.
      </Text>
      <List gap="xs">
        <Text variant="h2">Experimental</Text>
        <Text tone="quiet" size="compact">
          Early features that are still changing. Off unless you turn them on.
        </Text>
        {rows('experimental')}
      </List>
      <List gap="xs">
        <Text variant="h2">Optional</Text>
        <Text tone="quiet" size="compact">
          On by default. Turn any of them off for a quieter workspace.
        </Text>
        <List gap="l">
          {subsection('workspace')}
          {subsection('tickets')}
          {subsection('ambient')}
        </List>
      </List>
      <Text tone="quiet" size="compact">
        {ALWAYS_ON_AREAS}
      </Text>
      {message ? (
        <Text tone="danger" role="alert">
          {message}
        </Text>
      ) : null}
      <Row controlInsets="l">
        <Button type="button" action={SHELL_AND_GLOBAL_ACTIONS.restoreFeatureDefaults.value}>
          Restore Defaults
        </Button>
      </Row>
    </List>
  );
}
