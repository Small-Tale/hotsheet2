import { CatalogExample } from '@kerfjs/ui/catalog';
import { Select } from '@kerfjs/ui/select';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import type { CompatibilityAssessment } from '../compatibility';
import { ConnectionDetailsDialog } from '../components/connection-details-dialog';
import type { DemoModule, FormControl } from './demo-module';
import { DemoSettingsForm } from './demo-settings-form';
import { syncSettingsControls } from './settings-controls';

export type ConnectionDetailsScenario =
  'source-stale' | 'revision-mismatch' | 'server-too-old-safe' | 'server-too-old-manual' | 'client-too-old' | 'unknown';
export const connectionDetailsScenario = signal<ConnectionDetailsScenario>('source-stale');

const server = {
  generation: 'hs2',
  application_version: '0.1.0',
  build_revision: 'source-sha256:old',
  source_revision: 'source-sha256:current',
  source_stale: true,
  protocol: { min: 1, max: 1 },
  started_at: '2026-09-07T01:42:10Z',
} as const;

export function connectionDetailsAssessment(scenario: ConnectionDetailsScenario): CompatibilityAssessment {
  if (scenario === 'unknown')
    return {
      kind: 'unknown',
      detail: 'The server did not provide compatibility metadata.',
      revisionMismatch: false,
      sourceStale: false,
      canRestartServer: false,
      clientProtocol: { min: 1, max: 1 },
      clientRevision: 'source-sha256:client',
    };
  if (scenario === 'client-too-old')
    return {
      kind: 'client_too_old',
      detail: 'Client protocol 1–1 is older than server protocol 2–2.',
      revisionMismatch: true,
      sourceStale: false,
      canRestartServer: false,
      clientProtocol: { min: 1, max: 1 },
      clientRevision: 'source-sha256:client',
      server: { ...server, source_stale: false, source_revision: server.build_revision, protocol: { min: 2, max: 2 } },
    };
  if (scenario.startsWith('server-too-old'))
    return {
      kind: 'server_too_old',
      detail: 'Server protocol 0–0 is older than client protocol 1–1.',
      revisionMismatch: true,
      sourceStale: false,
      canRestartServer: scenario === 'server-too-old-safe',
      clientProtocol: { min: 1, max: 1 },
      clientRevision: 'source-sha256:client',
      server: { ...server, source_stale: false, source_revision: server.build_revision, protocol: { min: 0, max: 0 } },
    };
  if (scenario === 'revision-mismatch')
    return {
      kind: 'compatible',
      detail: 'The running server build differs from this development checkout.',
      revisionMismatch: true,
      sourceStale: false,
      canRestartServer: false,
      clientProtocol: { min: 1, max: 1 },
      clientRevision: 'source-sha256:client',
      server: { ...server, source_stale: false, source_revision: server.build_revision },
    };
  return {
    kind: 'compatible',
    detail: 'The running server build differs from this checkout.',
    revisionMismatch: true,
    sourceStale: true,
    canRestartServer: false,
    clientProtocol: { min: 1, max: 1 },
    clientRevision: 'source-sha256:client',
    server,
  };
}

export function resetConnectionDetailsDemo(root?: ParentNode) {
  connectionDetailsScenario.value = 'source-stale';
  if (root) syncSettingsControls(root, 'connection-details-dialog', { values: { scenario: 'source-stale' } });
}
export function ConnectionDetailsDialogDemo() {
  return (
    <CatalogExample label="Interactive" note="Choose a compatibility scenario in the settings pane.">
      <ConnectionDetailsDialog embedded assessment={connectionDetailsAssessment(connectionDetailsScenario.value)} />
    </CatalogExample>
  );
}
export function ConnectionDetailsDialogSettings() {
  return (
    <DemoSettingsForm settings="connection-details-dialog">
      <Select
        name="scenario"
        label="Compatibility Scenario"
        value={connectionDetailsScenario.value}
        choices={[
          { value: 'source-stale', label: 'Source build is stale.' },
          { value: 'revision-mismatch', label: 'Detached Compatible Build' },
          { value: 'server-too-old-safe', label: 'Server Too Old, Safe Restart' },
          { value: 'server-too-old-manual', label: 'Server Too Old, Manual Restart' },
          { value: 'client-too-old', label: 'Client Too Old' },
          { value: 'unknown', label: 'Metadata Unavailable' },
        ]}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </DemoSettingsForm>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireConnectionDetailsDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'change', '[data-settings="connection-details-dialog"] [name="scenario"]', (_event, target) => {
      connectionDetailsScenario.value = (target as FormControl).value as typeof connectionDetailsScenario.value;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'connection-details-dialog': {
      catalogExamples: true,
      Demo: ConnectionDetailsDialogDemo,
      Settings: ConnectionDetailsDialogSettings,
      reset: resetConnectionDetailsDemo,
    },
  },
  wire: wireConnectionDetailsDemo,
};
