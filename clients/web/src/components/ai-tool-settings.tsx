import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { Bot, Brain, Pencil, ShieldAlert, ShieldCheck } from 'lucide';

import { providerSelection } from '../ai-provider-defaults';
import type { AiToolDefaults } from '../api';
import { COMMANDS_AND_AI_ACTIONS } from '../interaction-attrs/commands-and-ai';
import type { AiToolDescriptor } from './drive-options-menu';
import { effortGaugeIcon } from './effort-gauge-icon';

/** The Select names of one provider's model and effort fields (HS2-EK24KF). */
export const aiProviderModelName = (tool: string) => `ai-provider-model-${tool}`;
export const aiProviderEffortName = (tool: string) => `ai-provider-effort-${tool}`;
export const aiProviderPermissionModeName = (tool: string) => `ai-provider-permission-mode-${tool}`;

/** A value that names no real model, used for the model menu's "Other…" choice. */
function otherModelValue(tool: AiToolDescriptor, modelId: string) {
  let value = '__hotsheet_other_model__';
  while (modelId === value || tool.models.some((item) => item.id === value)) value += '_';
  return value;
}

/** Every settings field shares one responsive column track, so model and effort wrap together on narrow panes. */
const FIELD_COLUMN = remify(224);

/** One provider's default model and effort, used whenever that provider is picked. */
function ProviderDefaults({
  tool,
  selection,
  isDefault,
}: {
  tool: AiToolDescriptor;
  selection: AiToolDefaults;
  isDefault: boolean;
}) {
  const modelId = selection.model ?? '',
    model = tool.models.find((item) => item.id === modelId),
    customModel = modelId && !model ? modelId : undefined,
    efforts = model?.effort_levels ?? [],
    other = otherModelValue(tool, modelId),
    modelChoices = [
      ...(customModel ? [{ value: customModel, label: customModel, icon: Brain, iconName: 'brain' }] : []),
      ...tool.models.map((item) => ({ value: item.id, label: item.label, icon: Brain, iconName: 'brain' })),
      { value: other, label: 'Other…', icon: Pencil, iconName: 'pencil', separatorBefore: true },
    ],
    // HS2-NHRDMW, HS2-53RV01: the provider's chat permission mode. Its hint describes the
    // selected mode; a mode with an explicit warning shows that warning instead.
    permissionModes = tool.permission_modes ?? [],
    permissionMode = permissionModes.find((mode) => mode.id === selection.permission_mode) ?? permissionModes.at(0);
  return (
    <section
      data-ai-provider={tool.id}
      data-ai-provider-default={String(isDefault)}
      data-other-model-value={other}
      data-permission-mode={permissionMode?.id}
      aria-label={`${tool.display_name} Defaults`}
    >
      <List gap="xs">
        <Text variant="h3" size="compact">
          {isDefault ? `${tool.display_name} (default)` : tool.display_name}
        </Text>
        <Grid minColumnWidth={FIELD_COLUMN} autoFill gap="m">
          <Select name={aiProviderModelName(tool.id)} label="Model" value={modelId} choices={modelChoices} />
          <Select
            name={aiProviderEffortName(tool.id)}
            label="Effort"
            value={selection.effort ?? efforts.at(0) ?? ''}
            disabled={!efforts.length}
            choices={efforts.map((value) => ({ value, label: value, icon: effortGaugeIcon(value), iconName: 'gauge' }))}
          />
          {permissionMode && (
            <Select
              name={aiProviderPermissionModeName(tool.id)}
              label="Chat Permissions"
              value={permissionMode.id}
              hint={permissionMode.warning ? `Warning: ${permissionMode.warning}` : permissionMode.description}
              choices={permissionModes.map((mode) => ({
                value: mode.id,
                label: mode.label,
                icon: mode.warning ? ShieldAlert : ShieldCheck,
                iconName: mode.warning ? 'shield-alert' : 'shield-check',
              }))}
            />
          )}
        </Grid>
      </List>
    </section>
  );
}

/**
 * Project Settings → AI tools (HS2-SW5S13, HS2-EK24KF): the project's default provider, then every
 * installed provider's own default model and effort.
 */
export function AiToolSettings({
  tools,
  selection,
  loading = false,
  message = '',
}: {
  tools: readonly AiToolDescriptor[];
  selection: AiToolDefaults;
  loading?: boolean;
  message?: string;
}) {
  if (!loading && !tools.length)
    return (
      <section data-component="ai-tool-settings">
        <EmptyState
          icon={<LucideIcon size="xl" icon={Bot} name="bot" />}
          title="No AI Tools Detected"
          detail="Install or enable a drivable AI-tool plugin to configure Drive, AI shells, and AI chat."
        />
      </section>
    );
  if (!tools.length)
    return (
      <section data-component="ai-tool-settings" aria-busy="true">
        <Text tone="quiet">Loading AI tools…</Text>
      </section>
    );
  const active = tools.find((tool) => tool.id === selection.tool) ?? tools.at(0)!,
    backups = (selection.fallback_tools ?? []).filter((id) => id !== active.id && tools.some((tool) => tool.id === id)),
    unused = tools.filter((tool) => tool.id !== active.id && !backups.includes(tool.id)),
    // A default provider that is no longer installed contributes neither its model nor its effort.
    base: AiToolDefaults =
      active.id === selection.tool ? selection : { tool: active.id, providers: selection.providers };
  return (
    <section data-component="ai-tool-settings" aria-busy={String(loading)}>
      <List gap="l">
        <Text tone="quiet" className="ai-tool-settings__intro">
          Choose this project's AI defaults on this machine. Each provider keeps its own model and effort.
        </Text>
        <Grid minColumnWidth={FIELD_COLUMN} autoFill className="ai-tool-settings__default">
          <Select
            name="ai-default-tool"
            label="Default AI Provider"
            hint="Used by Drive, AI shells, and AI chat unless you pick another provider."
            value={active.id}
            choices={tools.map((tool) => ({ value: tool.id, label: tool.display_name, icon: Bot, iconName: 'bot' }))}
          />
        </Grid>
        <section aria-label="Backup AI Providers">
          <List gap="xs">
            <Text variant="h3" size="compact">
              Backup AI Providers
            </Text>
            <Text tone="quiet">Automatic grouping tries these in order if an earlier provider cannot finish.</Text>
            {backups.map((id, index) => {
              const label = tools.find((tool) => tool.id === id)?.display_name ?? id;
              return (
                <Row gap="xs" wrap vAlign="middle" ariaLabel={`${label} backup controls`}>
                  <Text>{label}</Text>
                  <Button
                    action={COMMANDS_AND_AI_ACTIONS.moveAiBackupUp.value}
                    rootAttributes={{ 'data-tool': id }}
                    ariaLabel={`Move ${label} Up`}
                    size="compact"
                    emphasis="plain"
                  >
                    Up
                  </Button>
                  <Button
                    action={COMMANDS_AND_AI_ACTIONS.moveAiBackupDown.value}
                    rootAttributes={{ 'data-tool': id }}
                    ariaLabel={`Move ${label} Down`}
                    size="compact"
                    emphasis="plain"
                    disabled={index === backups.length - 1}
                  >
                    Down
                  </Button>
                  <Button
                    action={COMMANDS_AND_AI_ACTIONS.removeAiBackup.value}
                    rootAttributes={{ 'data-tool': id }}
                    ariaLabel={`Remove ${label} Backup`}
                    size="compact"
                    emphasis="plain"
                  >
                    Remove
                  </Button>
                </Row>
              );
            })}
            {unused.length > 0 && (
              <Select
                name="ai-add-backup-tool"
                label="Add Backup Provider"
                value=""
                choices={[
                  { value: '', label: 'Choose a Provider…' },
                  ...unused.map((tool) => ({ value: tool.id, label: tool.display_name, icon: Bot, iconName: 'bot' })),
                ]}
              />
            )}
          </List>
        </section>
        {tools.map((tool) => (
          <ProviderDefaults
            tool={tool}
            selection={providerSelection(base, tools, tool.id)}
            isDefault={tool.id === active.id}
          />
        ))}
        <Text tone="quiet" role="status">
          {message}
        </Text>
      </List>
    </section>
  );
}
