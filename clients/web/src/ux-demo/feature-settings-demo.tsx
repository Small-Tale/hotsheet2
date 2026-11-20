import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import { FeatureSettings } from '../components/feature-settings';
import { FEATURE_AREAS, type FeatureSettingsView, isFeatureId } from '../feature-areas';
import { SHELL_AND_GLOBAL_ACTIONS } from '../interaction-attrs/shell-and-global';
import type { DemoModule } from './demo-module';

/** A deterministic machine-wide view: every feature at its default unless a choice overrides it. */
function view(configured: Readonly<Record<string, boolean | null>>): FeatureSettingsView {
  const defaults = Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, area.defaultOn]));
  return {
    defaults,
    configured: Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, configured[area.id] ?? null])),
    effective: Object.fromEntries(
      FEATURE_AREAS.map((area) => [area.id, configured[area.id] ?? area.defaultOn] as const),
    ),
  };
}

/** The live example's choices; its switches and Restore Defaults change them as the server would. */
const choices = signal<Readonly<Record<string, boolean | null>>>({});

/** Restore the live example to its defaults. */
export function resetFeatureSettingsDemo(): void {
  choices.value = {};
}

/** App Settings › Features (HS2-3GJZGG) in its states. No server is involved. */
export function FeatureSettingsDemo() {
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Defaults, Live"
        note="Experimental areas are off and optional ones on. A switch changes its feature at once; Restore Defaults clears every choice."
      >
        <FeatureSettings settings={view(choices.value)} />
      </CatalogExample>
      <CatalogExample label="Quiet Workspace" note="Every optional area turned off.">
        <FeatureSettings
          settings={view(
            Object.fromEntries(FEATURE_AREAS.filter((area) => area.defaultOn).map((area) => [area.id, false])),
          )}
        />
      </CatalogExample>
      <CatalogExample label="Unread" note="Before the first read each feature shows its default.">
        <FeatureSettings />
      </CatalogExample>
      <CatalogExample label="Save Failed">
        <FeatureSettings settings={view({ drive: true })} message="settings.json is read-only" />
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'feature-settings': { catalogExamples: true, Demo: FeatureSettingsDemo, reset: resetFeatureSettingsDemo },
  },
  wire(root) {
    const scope = createScope();
    scope.add(
      delegate(root, 'change', SHELL_AND_GLOBAL_ACTIONS.setFeature.selector, (_event, target) => {
        const input = target as HTMLInputElement;
        if (isFeatureId(input.value)) choices.value = { ...choices.value, [input.value]: input.checked };
      }),
    );
    scope.add(
      delegate(root, 'click', SHELL_AND_GLOBAL_ACTIONS.restoreFeatureDefaults.selector, () => {
        resetFeatureSettingsDemo();
      }),
    );
    return () => {
      scope.dispose();
    };
  },
};
