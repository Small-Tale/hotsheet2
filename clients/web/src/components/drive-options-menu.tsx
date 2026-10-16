import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { PopupMenu } from '@kerfjs/ui/popup-menu';
import { RotateCcw } from 'lucide';

import { contextMenuAnchor } from '../context-menu-position';
import { providerModelEffortEntries } from './provider-model-effort-menu';

export interface AiToolModel {
  id: string;
  label: string;
  effort_levels?: readonly string[];
}
export interface AiToolPermissionMode {
  id: string;
  label: string;
  description?: string;
  warning?: string;
}
export interface AiToolDescriptor {
  id: string;
  display_name: string;
  models: readonly AiToolModel[];
  default_model?: string;
  default_effort?: string;
  actions?: readonly ('change_model' | 'change_effort' | 'change_permission_mode')[];
  permission_modes?: readonly AiToolPermissionMode[];
  default_permission_mode?: string;
}
export interface AiToolSelection {
  tool?: string;
  model?: string;
  effort?: string;
  permission_mode?: string;
}

export function DriveOptionsMenu({
  tools,
  selection,
  defaultSelection,
  loading = false,
  error,
  x = 0,
  y = 0,
}: {
  tools: readonly AiToolDescriptor[];
  selection: AiToolSelection;
  defaultSelection: AiToolSelection;
  loading?: boolean;
  error?: string;
  /** Viewport anchor of the menu's top-start corner (the drive row's top-left, HS2-2EHD8R). */
  x?: number;
  y?: number;
}) {
  if (!tools.length)
    return (
      <PopupMenu
        context
        anchor={contextMenuAnchor(x, y)}
        label="Drive Provider, Model, and Effort Options"
        placement="top-start"
        rootAttributes={{ 'data-context-menu': 'drive-options' }}
        items={[{ label: loading ? 'Detecting AI tools…' : error || 'No AI Tools Detected', disabled: true }]}
      />
    );
  const activeTool = tools.find((tool) => tool.id === (selection.tool ?? defaultSelection.tool)) ?? tools.at(0)!,
    modelId =
      selection.model ?? defaultSelection.model ?? activeTool.default_model ?? activeTool.models.at(0)?.id ?? '',
    activeModel = activeTool.models.find((model) => model.id === modelId),
    customModel = modelId && !activeModel ? modelId : undefined,
    efforts = activeModel?.effort_levels ?? [],
    currentEffort = selection.effort ?? defaultSelection.effort ?? activeTool.default_effort,
    defaultToolLabel = defaultSelection.tool
      ? (tools.find((tool) => tool.id === defaultSelection.tool)?.display_name ?? defaultSelection.tool)
      : undefined;
  return (
    <PopupMenu
      context
      anchor={contextMenuAnchor(x, y)}
      label="Drive Provider, Model, and Effort Options"
      placement="top-start"
      rootAttributes={{ 'data-context-menu': 'drive-options' }}
      items={[
        {
          label: 'Default',
          action: 'select-drive-default',
          checked: !selection.tool,
          icon: <LucideIcon icon={RotateCcw} name="rotate-ccw" />,
          details: defaultToolLabel ? <>{defaultToolLabel}</> : undefined,
        },
        { kind: 'divider' },
        ...providerModelEffortEntries({
          actions: {
            provider: 'select-drive-tool',
            model: 'select-drive-model',
            effort: 'select-drive-effort',
            manualModel: 'open-drive-manual-model',
          },
          providers: {
            choices: tools.map((tool) => ({ id: tool.id, label: tool.display_name })),
            currentId: activeTool.id,
            currentLabel: activeTool.display_name,
          },
          model: {
            choices: activeTool.models.map((model) => ({ id: model.id, label: model.label })),
            currentId: activeModel?.id,
            currentLabel: activeModel?.label ?? modelId,
            customModel,
          },
          effort: { efforts, current: currentEffort },
        }),
      ]}
    />
  );
}
