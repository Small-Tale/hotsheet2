import { signal } from 'kerfjs';

import { TagChip, type TagChipAppearance, type TagChipSize, type TagChipVariant } from '../components/tag-chip';
import { syncSettingsControls } from './settings-controls';

export const tagChipSettings = {
  label: signal('needs-design'),
  variant: signal<TagChipVariant>('neutral'),
  appearance: signal<TagChipAppearance>('filled'),
  size: signal<TagChipSize>('small'),
  removable: signal(true),
  pill: signal(false),
  disabled: signal(false),
  event: signal('No actions yet'),
};

export function syncTagChipSettingsControls(root: ParentNode): void {
  syncSettingsControls(root, 'tag-chip', {
    values: { label: tagChipSettings.label.value, variant: tagChipSettings.variant.value, appearance: tagChipSettings.appearance.value, size: tagChipSettings.size.value },
    checked: { removable: tagChipSettings.removable.value, pill: tagChipSettings.pill.value, disabled: tagChipSettings.disabled.value },
  });
}

export function resetTagChipDemo(controlRoot?: ParentNode): void {
  tagChipSettings.label.value = 'needs-design';
  tagChipSettings.variant.value = 'neutral';
  tagChipSettings.appearance.value = 'filled';
  tagChipSettings.size.value = 'small';
  tagChipSettings.removable.value = true;
  tagChipSettings.pill.value = false;
  tagChipSettings.disabled.value = false;
  tagChipSettings.event.value = 'No actions yet';
  if (controlRoot) syncTagChipSettingsControls(controlRoot);
}

export function TagChipDemo() {
  return (
    <section class="component-stage" aria-label="TagChip demo">
      <div class="component-stage__canvas">
        {TagChip({
          id: 'demo-tag', label: tagChipSettings.label.value,
          variant: tagChipSettings.variant.value, appearance: tagChipSettings.appearance.value,
          size: tagChipSettings.size.value, removable: tagChipSettings.removable.value,
          pill: tagChipSettings.pill.value, disabled: tagChipSettings.disabled.value,
        })}
      </div>
      <p class="component-stage__event" aria-live="polite">{tagChipSettings.event}</p>
      <p class="component-stage__guidance">
        The owning feature handles removal and ticket mutation. TagChip emits a semantic
        remove event with stable tag identity.
      </p>
    </section>
  );
}

export function TagChipSettings() {
  return (
    <form class="settings-form" data-settings="tag-chip">
      <wa-input name="label" label="Label" value={tagChipSettings.label.value}></wa-input>
      <wa-select name="variant" label="Variant" value={tagChipSettings.variant.value}>
        {(['neutral', 'brand', 'success', 'warning', 'danger'] as const).map(value => <wa-option value={value}>{value}</wa-option>)}
      </wa-select>
      <wa-select name="appearance" label="Appearance" value={tagChipSettings.appearance.value}>
        {(['filled-outlined', 'filled', 'outlined', 'accent'] as const).map(value => <wa-option value={value}>{value}</wa-option>)}
      </wa-select>
      <wa-select name="size" label="Size" value={tagChipSettings.size.value}>
        {(['small', 'medium', 'large'] as const).map(value => <wa-option value={value}>{value}</wa-option>)}
      </wa-select>
      <wa-checkbox name="removable" checked={tagChipSettings.removable.value}>Removable</wa-checkbox>
      <wa-checkbox name="pill" checked={tagChipSettings.pill.value}>Pill shape</wa-checkbox>
      <wa-checkbox name="disabled" checked={tagChipSettings.disabled.value}>Disabled</wa-checkbox>
      <wa-button type="button" data-action="reset-settings">Reset</wa-button>
    </form>
  );
}
