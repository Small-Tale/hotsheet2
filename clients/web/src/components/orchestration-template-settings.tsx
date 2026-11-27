import { Badge } from '@kerfjs/ui/badge';
import { Button } from '@kerfjs/ui/button';
import { rem } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Archive, LayoutTemplate, MoreHorizontal, Pencil, Plus } from 'lucide';

import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';
import { templateSummary } from '../orchestration-template-editor';

/** One checked-in template as the settings list shows it. */
export interface TemplateSettingsRow {
  templateId: string;
  name: string;
  description: string;
  retired: boolean;
  /** "2 inputs · 3 steps · version 4". */
  summary: string;
}

export interface OrchestrationTemplateSettingsProps {
  rows?: readonly TemplateSettingsRow[];
  loading?: boolean;
  /** A read failure, in words. */
  error?: string;
  /** Templates with a retire on its way. */
  busy?: readonly string[];
}

/** The settings rows of the server's template list: active templates first, each group by name. */
export function templateSettingsRows(templates: readonly Record<string, unknown>[]): TemplateSettingsRow[] {
  return templates
    .flatMap((template) => {
      const id = typeof template.template_id === 'string' ? template.template_id : '';
      if (!id) return [];
      return [
        {
          templateId: id,
          name: typeof template.name === 'string' && template.name.trim() ? template.name : id,
          description: typeof template.description === 'string' ? template.description : '',
          retired: template.state === 'retired',
          summary: templateSummary(template),
        },
      ];
    })
    .sort((a, b) => Number(a.retired) - Number(b.retired) || a.name.localeCompare(b.name));
}

const menuIcon = <LucideIcon icon={MoreHorizontal} name="more-horizontal" />;
const templateIcon = <LucideIcon icon={LayoutTemplate} name="layout-template" />;

/**
 * HS2-1XCW6N: Settings › Orchestration › Templates. The checked-in plan templates Use Template… applies,
 * each a row that opens the visual editor, with Retire Template… in its menu; New Template opens a blank
 * one. Built like the Projects section above it.
 */
export function OrchestrationTemplateSettings({
  rows = [],
  loading = false,
  error,
  busy = [],
}: OrchestrationTemplateSettingsProps) {
  return (
    <section aria-label="Templates" data-component="orchestration-template-settings">
      <List gap="m">
        <List gap="xs">
          <Text variant="h2" size="large" flush>
            Templates
          </Text>
          <Text tone="quiet" flush>
            Use Template… in the Orchestration view applies these. They are checked in with the project at
            .hotsheet2/orchestration-templates.json.
          </Text>
        </List>
        <Row>
          <Button
            emphasis="secondary"
            icon={<LucideIcon icon={Plus} name="plus" />}
            action={ACTIONS.newTemplate.value}
            disabled={loading && rows.length === 0}
          >
            New Template
          </Button>
        </Row>
        {error ? (
          <Text tone="danger" size="compact" role="alert" flush>
            {error}
          </Text>
        ) : null}
        {loading && rows.length === 0 ? (
          <List gap={rem(0.125)}>
            <ListActionRow
              label="Loading"
              action={ACTIONS.editTemplate.value}
              placeholder
              trailingMenu={{ label: 'Template Actions', icon: menuIcon, items: [] }}
            />
          </List>
        ) : rows.length === 0 ? (
          <EmptyState
            icon={templateIcon}
            title="No templates yet."
            detail="Create one here, or save a goal’s plan with Save a Goal as Template… in Use Template…."
            rootAttributes={{ 'data-orchestration-templates-empty': '' }}
          />
        ) : (
          <List gap={rem(0.125)}>
            {rows.map((row) => {
              const attributes = { 'data-template-id': row.templateId };
              return (
                <ListActionRow
                  label={row.name}
                  description={[row.summary, row.description].filter(Boolean).join(' · ')}
                  status={
                    row.retired ? (
                      <Badge tone="neutral" appearance="quiet" size="compact">
                        Retired
                      </Badge>
                    ) : undefined
                  }
                  icon={templateIcon}
                  iconTile="neutral"
                  action={ACTIONS.editTemplate.value}
                  itemId={row.templateId}
                  busy={busy.includes(row.templateId)}
                  rootAttributes={{ 'data-orchestration-template': row.templateId }}
                  trailingMenu={{
                    label: `Actions for ${row.name}`,
                    icon: menuIcon,
                    items: [
                      {
                        label: row.retired ? 'Show Template' : 'Edit Template…',
                        action: ACTIONS.editTemplate.value,
                        icon: <LucideIcon icon={Pencil} name="pencil" />,
                        attributes,
                      },
                      { kind: 'divider' },
                      {
                        label: 'Retire Template…',
                        action: ACTIONS.retireTemplate.value,
                        tone: 'danger',
                        icon: <LucideIcon icon={Archive} name="archive" />,
                        disabled: row.retired || busy.includes(row.templateId),
                        disabledReason: row.retired ? 'This template is already retired.' : undefined,
                        attributes,
                      },
                    ],
                  }}
                />
              );
            })}
          </List>
        )}
      </List>
    </section>
  );
}
