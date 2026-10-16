import type { AiToolDefaults } from './api';
import type { AiToolDescriptor } from './components/drive-options-menu';

/**
 * Per-provider AI defaults (HS2-EK24KF). A project picks a default provider, and every provider
 * keeps its own default model and effort, used whenever that provider is chosen without an explicit
 * model (Settings, the Drive menu, AI shells, commands). The default provider's entry mirrors the
 * top-level model and effort.
 */

/**
 * The permission mode a provider runs under (HS2-NHRDMW): the first candidate the tool declares,
 * else its declared default; `undefined` for a tool without selectable modes.
 */
export function compatiblePermissionMode(
  descriptor: Pick<AiToolDescriptor, 'permission_modes' | 'default_permission_mode'> | undefined,
  ...candidates: readonly (string | undefined)[]
): string | undefined {
  const modes = descriptor?.permission_modes ?? [];
  if (!modes.length) return undefined;
  return (
    candidates.find((candidate) => candidate && modes.some((mode) => mode.id === candidate)) ??
    (modes.some((mode) => mode.id === descriptor?.default_permission_mode)
      ? descriptor?.default_permission_mode
      : modes[0]?.id)
  );
}

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
          : efforts[0],
    permissionMode = compatiblePermissionMode(
      descriptor,
      saved?.permission_mode,
      isDefault ? defaults.permission_mode : undefined,
    );
  return {
    tool,
    ...(model ? { model } : {}),
    ...(effort ? { effort } : {}),
    ...(permissionMode ? { permission_mode: permissionMode } : {}),
  };
}

/**
 * Save one provider's model and effort (and permission mode, HS2-NHRDMW); the default provider's
 * change also updates the top level. A selection without `permission_mode` keeps the saved one.
 */
export function withProviderSelection(
  defaults: AiToolDefaults,
  tool: string,
  selection: { model?: string; effort?: string; permission_mode?: string },
): AiToolDefaults {
  const permissionMode =
      'permission_mode' in selection
        ? selection.permission_mode
        : (defaults.providers?.[tool]?.permission_mode ??
          (tool === defaults.tool ? defaults.permission_mode : undefined)),
    entry = {
      ...(selection.model ? { model: selection.model } : {}),
      ...(selection.effort ? { effort: selection.effort } : {}),
      ...(permissionMode ? { permission_mode: permissionMode } : {}),
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
        ...(defaults.permission_mode || defaults.providers?.[defaults.tool]?.permission_mode
          ? { permission_mode: defaults.providers?.[defaults.tool]?.permission_mode ?? defaults.permission_mode }
          : {}),
      },
    },
    // The resolved permission mode stays implicit: only an explicit saved choice is carried, so a
    // later plugin default change still reaches a project that never picked a mode (HS2-NHRDMW).
    { model, effort } = providerSelection({ ...defaults, providers }, tools, tool);
  // The outgoing default's top-level mode now lives in its own entry; it must not leak to `tool`.
  return withProviderSelection({ ...defaults, tool, permission_mode: undefined, providers }, tool, {
    ...(model ? { model } : {}),
    ...(effort ? { effort } : {}),
  });
}
