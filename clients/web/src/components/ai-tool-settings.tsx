import './ai-tool-settings.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { Bot, Brain, Gauge, Pencil } from 'lucide';

import { providerSelection } from '../ai-provider-defaults';
import type { AiToolDefaults } from '../api';
import type { AiToolDescriptor } from './drive-options-menu';

/** The Select names of one provider's model and effort fields (HS2-EK24KF). */
export const aiProviderModelName = (tool: string) => `ai-provider-model-${tool}`;
export const aiProviderEffortName = (tool: string) => `ai-provider-effort-${tool}`;

/** A value that names no real model, used for the model menu's "Other…" choice. */
function otherModelValue(tool: AiToolDescriptor, modelId: string) {
  let value = '__hotsheet_other_model__';
  while (modelId === value || tool.models.some((item) => item.id === value)) value += '_';
  return value;
}

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
    ];
  return (
    <section
      class="ai-tool-settings__provider"
      data-ai-provider={tool.id}
      data-ai-provider-default={String(isDefault)}
      data-other-model-value={other}
      aria-label={`${tool.display_name} defaults`}
    >
      <Text variant="h3" size="compact">
        {isDefault ? `${tool.display_name} (default)` : tool.display_name}
      </Text>
      <div class="ai-tool-settings__grid">
        <Select name={aiProviderModelName(tool.id)} label="Model" value={modelId} choices={modelChoices} />
        <Select
          name={aiProviderEffortName(tool.id)}
          label="Effort"
          value={selection.effort ?? efforts.at(0) ?? ''}
          disabled={!efforts.length}
          choices={efforts.map((value) => ({ value, label: value, icon: Gauge, iconName: 'gauge' }))}
        />
      </div>
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
      <section class="ai-tool-settings" data-component="ai-tool-settings">
        <div class="ai-tool-settings__empty">
          <LucideIcon icon={Bot} name="bot" />
          <strong>No AI tools detected</strong>
          <p>Install or enable a drivable AI-tool plugin to configure Drive, AI shells, and AI chat.</p>
        </div>
      </section>
    );
  if (!tools.length)
    return (
      <section class="ai-tool-settings" data-component="ai-tool-settings" aria-busy="true">
        <p>Loading AI tools…</p>
      </section>
    );
  const active = tools.find((tool) => tool.id === selection.tool) ?? tools.at(0)!,
    // A default provider that is no longer installed contributes neither its model nor its effort.
    base: AiToolDefaults =
      active.id === selection.tool ? selection : { tool: active.id, providers: selection.providers };
  return (
    <section class="ai-tool-settings" data-component="ai-tool-settings" aria-busy={String(loading)}>
      <p>Choose this project's AI defaults on this machine. Each provider keeps its own model and effort.</p>
      <div class="ai-tool-settings__default">
        <Select
          name="ai-default-tool"
          label="Default AI provider"
          hint="Used by Drive, AI shells, and AI chat unless you pick another provider."
          value={active.id}
          choices={tools.map((tool) => ({ value: tool.id, label: tool.display_name, icon: Bot, iconName: 'bot' }))}
        />
      </div>
      {tools.map((tool) => (
        <ProviderDefaults
          tool={tool}
          selection={providerSelection(base, tools, tool.id)}
          isDefault={tool.id === active.id}
        />
      ))}
      <p role="status">{message}</p>
    </section>
  );
}
