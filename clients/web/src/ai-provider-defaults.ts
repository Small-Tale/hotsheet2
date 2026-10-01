import type { AiToolDefaults } from './api';
import type { AiToolDescriptor } from './components/drive-options-menu';

/**
 * Per-provider AI defaults (HS2-EK24KF). A project picks a default provider, and every provider
 * keeps its own default model and effort, used whenever that provider is chosen without an explicit
 * model (Settings, the Drive menu, AI shells, commands). The default provider's entry mirrors the
 * top-level model and effort.
 */

/** A provider's resolved selection: its saved model and effort, else the manifest defaults. */
export function providerSelection(
  defaults: AiToolDefaults,
  tools: readonly AiToolDescriptor[],
  tool: string,
): AiToolDefaults {
  const descriptor = tools.find((item) => item.id === tool),
    saved = defaults.providers?.[tool],
    isDefault = tool === defaults.tool,
    model =
      saved?.model ??
      (isDefault ? defaults.model : undefined) ??
      descriptor?.default_model ??
      descriptor?.models[0]?.id,
    known = descriptor?.models.find((item) => item.id === model),
    efforts = known?.effort_levels ?? [],
    requested = saved?.effort ?? (isDefault && model === defaults.model ? defaults.effort : undefined),
    effort =
      requested && (!known || efforts.includes(requested))
        ? requested
        : descriptor?.default_effort && efforts.includes(descriptor.default_effort)
          ? descriptor.default_effort
          : efforts[0];
  return { tool, ...(model ? { model } : {}), ...(effort ? { effort } : {}) };
}

/** Save one provider's model and effort; the default provider's change also updates the top level. */
export function withProviderSelection(
  defaults: AiToolDefaults,
  tool: string,
  selection: { model?: string; effort?: string },
): AiToolDefaults {
  const entry = {
      ...(selection.model ? { model: selection.model } : {}),
      ...(selection.effort ? { effort: selection.effort } : {}),
    },
    providers = { ...defaults.providers, [tool]: entry };
  if (tool !== defaults.tool) return { ...defaults, providers };
  return { tool, ...entry, providers };
}

/** Make `tool` the default provider, carrying its own saved model and effort to the top level. */
export function withDefaultProvider(
  defaults: AiToolDefaults,
  tools: readonly AiToolDescriptor[],
  tool: string,
): AiToolDefaults {
  const current = providerSelection(defaults, tools, defaults.tool),
    providers = {
      ...defaults.providers,
      // Keep the outgoing default's choice as its own entry so switching back restores it.
      [defaults.tool]: {
        ...(current.model ? { model: current.model } : {}),
        ...(current.effort ? { effort: current.effort } : {}),
      },
    },
    next = providerSelection({ ...defaults, providers }, tools, tool);
  return withProviderSelection({ ...defaults, tool, providers }, tool, next);
}
