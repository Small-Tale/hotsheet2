import { Button } from '@kerfjs/ui/button';
import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';
import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  GlanceGauge,
  type GlanceGaugePalette,
  type GlanceGaugeSize,
  type GlanceGaugeTone,
  type GlanceGaugeVariant,
} from '../components/glance-gauge';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

/** Synthetic readings (HS2-CVE3SK): each series is scaled to its own 24-hour peak. */
const TOKENS = { label: 'Token rate', unit: 'tokens/min', peak: 6200 } as const;
const TICKETS = { label: 'Tickets updated', unit: 'tickets/h', peak: 22 } as const;

/** Reading as a share of the 24-hour peak; 140% shows a reading above the peak pinned to the end. */
const READING_CHOICES: SelectChoice[] = [
  { value: '0', label: '0% (Idle)' },
  { value: '26', label: '26% (Typical)' },
  { value: '64', label: '64% (Busy)' },
  { value: '100', label: '100% (At Peak)' },
  { value: '140', label: '140% (Above Peak, Pinned)' },
];

const DEFAULTS = {
  reading: '26',
  variant: 'full' as GlanceGaugeVariant,
  tone: 'brand' as GlanceGaugeTone,
  palette: 'theme' as GlanceGaugePalette,
  size: 'large' as GlanceGaugeSize,
};

export const glanceGaugeSettings = {
  reading: signal<string>(DEFAULTS.reading),
  variant: signal<GlanceGaugeVariant>(DEFAULTS.variant),
  tone: signal<GlanceGaugeTone>(DEFAULTS.tone),
  palette: signal<GlanceGaugePalette>(DEFAULTS.palette),
  size: signal<GlanceGaugeSize>(DEFAULTS.size),
};

export function resetGlanceGaugeDemo(root?: ParentNode): void {
  glanceGaugeSettings.reading.value = DEFAULTS.reading;
  glanceGaugeSettings.variant.value = DEFAULTS.variant;
  glanceGaugeSettings.tone.value = DEFAULTS.tone;
  glanceGaugeSettings.palette.value = DEFAULTS.palette;
  glanceGaugeSettings.size.value = DEFAULTS.size;
  if (root) syncSettingsControls(root, 'glance-gauge', { values: { ...DEFAULTS } });
}

function series(tone: GlanceGaugeTone) {
  return tone === 'success' ? TICKETS : TOKENS;
}

/** The interactive specimen plus the fixed states the glance surfaces use. */
export function GlanceGaugeDemo() {
  const tone = glanceGaugeSettings.tone.value;
  const { label, unit, peak } = series(tone);
  const value = Math.round((peak * Number(glanceGaugeSettings.reading.value)) / 100);
  const interactive = GlanceGauge({
    value,
    peak,
    label,
    unit,
    variant: glanceGaugeSettings.variant.value,
    tone,
    palette: glanceGaugeSettings.palette.value,
    size: glanceGaugeSettings.size.value,
  });
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Interactive"
        note="Change the reading to watch the needle sweep; reduced motion jumps straight to it. Hover for the title, which is also the accessible name."
      >
        {glanceGaugeSettings.palette.value === 'standby' ? (
          <div class="demo-tone-surface" data-surface="standby">
            {interactive}
          </div>
        ) : (
          interactive
        )}
      </CatalogExample>
      <CatalogExample
        label="Bare at Chip Height"
        note="The compact project sidebar and grid sidebar pages: no text on the face. Blue is tokens/min, green is tickets updated/h."
      >
        <Row gap="xs" vAlign="middle" wrap>
          {GlanceGauge({ ...TOKENS, value: 1600, variant: 'bare', tone: 'brand' })}
          {GlanceGauge({ ...TICKETS, value: 14, variant: 'bare', tone: 'success' })}
          {GlanceGauge({ ...TOKENS, value: 1600, variant: 'bare', tone: 'brand', size: 'medium' })}
          {GlanceGauge({ ...TICKETS, value: 14, variant: 'bare', tone: 'success', size: 'medium' })}
        </Row>
      </CatalogExample>
      <CatalogExample
        label="Full: Idle, Mid, and Above Peak"
        note="Stats Right now tiles. A reading above its 24-hour peak pins to the end."
      >
        <Row gap="m" vAlign="middle" wrap>
          {GlanceGauge({ ...TOKENS, value: 0 })}
          {GlanceGauge({ ...TOKENS, value: 1600 })}
          {GlanceGauge({ ...TICKETS, value: 31, tone: 'success' })}
        </Row>
      </CatalogExample>
      <CatalogExample label="Standby" note="Very large dark red on black; the standby palette overrides both tones.">
        <div class="demo-tone-surface" data-surface="standby">
          {GlanceGauge({ ...TOKENS, peak: 9800, value: 1600, palette: 'standby', size: 'fill' })}
        </div>
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export function GlanceGaugeSettings() {
  return (
    <DemoSettingsForm settings="glance-gauge">
      <Select name="reading" label="Reading" value={glanceGaugeSettings.reading.value} choices={READING_CHOICES} />
      <Select
        name="variant"
        label="Variant"
        value={glanceGaugeSettings.variant.value}
        choices={[
          { value: 'full', label: 'Full (Value in Dial)' },
          { value: 'bare', label: 'Bare (Title Only)' },
        ]}
      />
      <Select
        name="tone"
        label="Meaning"
        value={glanceGaugeSettings.tone.value}
        choices={[
          { value: 'brand', label: 'Tokens per Minute (Blue)' },
          { value: 'success', label: 'Tickets Updated per Hour (Green)' },
        ]}
      />
      <Select
        name="palette"
        label="Palette"
        value={glanceGaugeSettings.palette.value}
        choices={[
          { value: 'theme', label: 'Theme' },
          { value: 'standby', label: 'Standby (Dark Red on Black)' },
        ]}
      />
      <Select
        name="size"
        label="Size"
        value={glanceGaugeSettings.size.value}
        choices={[
          { value: 'chip', label: 'Chip (40px)' },
          { value: 'medium', label: 'Medium (88px)' },
          { value: 'large', label: 'Large (120px)' },
          { value: 'fill', label: 'Fill Container' },
        ]}
      />
      <Button type="button" action={'reset-settings'}>
        Reset
      </Button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireGlanceGaugeDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="glance-gauge"] [name]', (_event, target) => {
      const control = target as FormControl;
      const name = control.getAttribute('name');
      if (name === 'reading') glanceGaugeSettings.reading.value = control.value;
      if (name === 'variant') glanceGaugeSettings.variant.value = control.value as GlanceGaugeVariant;
      if (name === 'tone') glanceGaugeSettings.tone.value = control.value as GlanceGaugeTone;
      if (name === 'palette') glanceGaugeSettings.palette.value = control.value as GlanceGaugePalette;
      if (name === 'size') glanceGaugeSettings.size.value = control.value as GlanceGaugeSize;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders. */
export const demoModule: DemoModule = {
  entries: {
    'glance-gauge': {
      catalogExamples: true,
      Demo: GlanceGaugeDemo,
      Settings: GlanceGaugeSettings,
      reset: resetGlanceGaugeDemo,
    },
  },
  wire: wireGlanceGaugeDemo,
};
