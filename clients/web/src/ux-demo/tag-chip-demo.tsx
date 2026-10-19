import { Button } from '@kerfjs/ui/button';
import { CatalogExample } from '@kerfjs/ui/catalog';
import { Checkbox } from '@kerfjs/ui/checkbox';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { TAG_CHIP_REMOVE_ACTION } from '../components/tag-chip';
import { TagChip, type TagChipAppearance, type TagChipSize, type TagChipVariant } from '../components/tag-chip';
import { DemoEvent } from './components/demo-event';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
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
    values: {
      label: tagChipSettings.label.value,
      variant: tagChipSettings.variant.value,
      appearance: tagChipSettings.appearance.value,
      size: tagChipSettings.size.value,
    },
    checked: {
      removable: tagChipSettings.removable.value,
      pill: tagChipSettings.pill.value,
      disabled: tagChipSettings.disabled.value,
    },
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
    <>
      <CatalogExample
        align="glyph"
        label="Interactive"
        rootAttributes={{ 'data-tag-chip-demo': '' }}
        note="The owning feature handles removal and ticket mutation. The chip's delegated remove action carries the stable tag identity."
      >
        {TagChip({
          id: 'demo-tag',
          label: tagChipSettings.label.value,
          variant: tagChipSettings.variant.value,
          appearance: tagChipSettings.appearance.value,
          size: tagChipSettings.size.value,
          removable: tagChipSettings.removable.value,
          pill: tagChipSettings.pill.value,
          disabled: tagChipSettings.disabled.value,
        })}
      </CatalogExample>
      <DemoEvent>{tagChipSettings.event.value}</DemoEvent>
    </>
  );
}

export function TagChipSettings() {
  return (
    <DemoSettingsForm settings="tag-chip">
      <wa-input name="label" label="Label" value={tagChipSettings.label.value} />
      <Select
        name="variant"
        label="Variant"
        value={tagChipSettings.variant.value}
        choices={(['neutral', 'brand', 'success', 'warning', 'danger'] as const).map((value) => ({
          value,
          label: value,
        }))}
      />
      <Select
        name="appearance"
        label="Appearance"
        value={tagChipSettings.appearance.value}
        choices={(['filled-outlined', 'filled', 'outlined', 'accent'] as const).map((value) => ({
          value,
          label: value,
        }))}
      />
      <Select
        name="size"
        label="Size"
        value={tagChipSettings.size.value}
        choices={(['small', 'medium', 'large'] as const).map((value) => ({ value, label: value }))}
      />
      <Checkbox name="removable" checked={tagChipSettings.removable.value} label="Removable" />
      <Checkbox name="pill" checked={tagChipSettings.pill.value} label="Pill shape" />
      <Checkbox name="disabled" checked={tagChipSettings.disabled.value} label="Disabled" />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireTagChipDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'input', '[data-settings="tag-chip"] [name="label"]', (_event, target) => {
      tagChipSettings.label.value = (target as FormControl).value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="tag-chip"] [name]', (_event, target) => {
      const control = target as FormControl;
      switch (control.getAttribute('name')) {
        case 'variant':
          tagChipSettings.variant.value = control.value as typeof tagChipSettings.variant.value;
          break;
        case 'appearance':
          tagChipSettings.appearance.value = control.value as typeof tagChipSettings.appearance.value;
          break;
        case 'size':
          tagChipSettings.size.value = control.value as typeof tagChipSettings.size.value;
          break;
        case 'removable':
          tagChipSettings.removable.value = control.checked;
          break;
        case 'pill':
          tagChipSettings.pill.value = control.checked;
          break;
        case 'disabled':
          tagChipSettings.disabled.value = control.checked;
          break;
        case null:
          break;
      }
    }),
  );
  demoListeners.add(
    delegate(root, 'click', `[data-tag-chip-demo] [data-action="${TAG_CHIP_REMOVE_ACTION}"]`, (_event, target) => {
      const chip = target.closest<HTMLElement>('[data-component="chip"]');
      if (chip && chip.dataset.disabled === undefined)
        tagChipSettings.event.value = `Remove requested for ${chip.dataset.itemId}`;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'tag-chip': { catalogExamples: true, Demo: TagChipDemo, Settings: TagChipSettings, reset: resetTagChipDemo },
  },
  wire: wireTagChipDemo,
};
