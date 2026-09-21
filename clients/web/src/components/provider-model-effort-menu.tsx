import '@awesome.me/webawesome/dist/components/dropdown-item/dropdown-item.js';
import '@awesome.me/webawesome/dist/components/divider/divider.js';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Bot, Brain, Gauge, type IconNode, Pencil } from 'lucide';

/** One id/label choice a submenu offers (a provider or a model). */
export interface ProviderModelEffortChoice {
  id: string;
  label: string;
}

/** The Provider/Model/Effort submenu group, shared by every surface that lets a user pick
 * an AI provider, model, and optional effort (the Drive options popup and the in-conversation
 * model/effort popup, and the command-editor field). The caller owns
 * the enclosing `<wa-dropdown>` (its trigger, placement, and any leading rows such as Drive's
 * "Default" reset), and each submenu is included only when its data is supplied — so a surface
 * shows exactly the choices it supports. Selections are reported as delegated host-level
 * `click`s carrying `data-action`/`data-value`, matching the existing handlers. */
export interface ProviderModelEffortSubmenusProps {
  /** Host-level actions each submenu stamps onto its rows. */
  actions: { provider?: string; model: string; effort: string; manualModel: string };
  /** Provider submenu; omit to hide it (a single provider, or a read-only surface). */
  providers?: { choices: readonly ProviderModelEffortChoice[]; currentId?: string; currentLabel: string };
  /** Model submenu (with an optional ephemeral custom model row and the "Other…" entry);
   * omit to hide it. */
  model?: {
    choices: readonly ProviderModelEffortChoice[];
    currentId?: string;
    currentLabel: string;
    /** A manually entered model id that is not in the catalog, shown as its own checked row. */
    customModel?: string;
  };
  /** Effort submenu; omit to hide it. When present but `efforts` is empty the parent item is
   * shown disabled (the Drive behavior), so a model without effort levels still lists Effort. */
  effort?: { efforts: readonly string[]; current?: string };
}

/** One selectable submenu row. Exported for surfaces that compose extra rows in the same style. */
export function providerModelEffortChoice(
  action: string,
  value: string,
  label: string,
  selected: boolean,
  icon: IconNode,
  iconName: string,
) {
  return (
    <wa-dropdown-item
      slot="submenu"
      aria-current={selected ? 'true' : undefined}
      data-action={action}
      data-value={value}
      value={value}
    >
      <span slot="icon">
        <LucideIcon icon={icon} name={iconName} />
      </span>
      {label}
    </wa-dropdown-item>
  );
}

export function ProviderModelEffortSubmenus({ actions, providers, model, effort }: ProviderModelEffortSubmenusProps) {
  return (
    <>
      {providers && actions.provider && (
        <wa-dropdown-item>
          <span slot="icon">
            <LucideIcon icon={Bot} name="bot" />
          </span>
          Provider<span slot="details">{providers.currentLabel}</span>
          {providers.choices.map((choice) =>
            providerModelEffortChoice(
              actions.provider!,
              choice.id,
              choice.label,
              choice.id === providers.currentId,
              Bot,
              'bot',
            ),
          )}
        </wa-dropdown-item>
      )}
      {model && (
        <wa-dropdown-item>
          <span slot="icon">
            <LucideIcon icon={Brain} name="brain" />
          </span>
          Model<span slot="details">{model.currentLabel}</span>
          {model.customModel &&
            providerModelEffortChoice(actions.model, model.customModel, model.customModel, true, Brain, 'brain')}
          {model.choices.map((choice) =>
            providerModelEffortChoice(
              actions.model,
              choice.id,
              choice.label,
              choice.id === model.currentId,
              Brain,
              'brain',
            ),
          )}
          <wa-divider slot="submenu"></wa-divider>
          {providerModelEffortChoice(actions.manualModel, 'other', 'Other…', false, Pencil, 'pencil')}
        </wa-dropdown-item>
      )}
      {effort && (
        <wa-dropdown-item disabled={!effort.efforts.length}>
          <span slot="icon">
            <LucideIcon icon={Gauge} name="gauge" />
          </span>
          Effort<span slot="details">{effort.current}</span>
          {effort.efforts.map((value) =>
            providerModelEffortChoice(actions.effort, value, value, value === effort.current, Gauge, 'gauge'),
          )}
        </wa-dropdown-item>
      )}
    </>
  );
}
