import type { DemoEntry } from './demo-module';

/** The fixed settings panel for the selected entry's `Settings` form (HS2-HMVBZW). */
export function SettingsInspector({ name, entry }: { name: string; entry: DemoEntry | undefined }) {
  const Settings = entry?.Settings;
  return (
    <aside class="settings-inspector" aria-label={`${name} settings`}>
      <header>
        <div>
          <p class="eyebrow">Demo settings</p>
          <h2 class="settings-inspector__title">{name}</h2>
        </div>
        <wa-button class="settings-toggle" data-action="toggle-settings" aria-expanded="true">
          Close settings
        </wa-button>
      </header>
      {Settings ? <Settings /> : <p>This demo has no adjustable settings.</p>}
    </aside>
  );
}
