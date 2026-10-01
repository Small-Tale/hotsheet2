import { Select, type SelectChoice } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import type { CalibrationReport } from '../api';
import { ConfidenceCalibration, type ConfidenceCalibrationState } from '../components/confidence-calibration';
import { syncSettingsControls } from './settings-controls';

/** A deterministic report covering every band shape: rated, unresolved, empty, and unscored. */
const CALIBRATION_FIXTURE: CalibrationReport = {
  completions: 7,
  scored: 5,
  bands: [
    {
      band: 'verified',
      range: '90-100',
      completions: 2,
      verified: 2,
      reopened: 0,
      pending: 0,
      reopen_rate: 0,
      mean_confidence: 93.5,
    },
    {
      band: 'assumed',
      range: '70-89',
      completions: 2,
      verified: 1,
      reopened: 1,
      pending: 0,
      reopen_rate: 0.5,
      mean_confidence: 80,
    },
    {
      band: 'partial',
      range: '40-69',
      completions: 1,
      verified: 0,
      reopened: 0,
      pending: 1,
      mean_confidence: 55,
    },
    { band: 'unverified', range: '0-39', completions: 0, verified: 0, reopened: 0, pending: 0 },
    { band: 'unscored', range: '-', completions: 2, verified: 0, reopened: 1, pending: 1, reopen_rate: 1 },
  ],
  events: [
    { slug: 'HS2-K4N7QX', completed_at: '2026-09-20T09:00:00Z', confidence: 95, outcome: 'verified' },
    { slug: 'HS2-R2D8WM', completed_at: '2026-09-21T09:00:00Z', confidence: 92, outcome: 'verified' },
    { slug: 'HS2-B6T1PZ', completed_at: '2026-09-22T09:00:00Z', confidence: 82, outcome: 'reopened' },
    { slug: 'HS2-B6T1PZ', completed_at: '2026-09-24T09:00:00Z', confidence: 78, outcome: 'verified' },
    { slug: 'HS2-H9Q3LV', completed_at: '2026-09-25T09:00:00Z', outcome: 'reopened' },
    { slug: 'HS2-F5Y2JC', completed_at: '2026-09-28T09:00:00Z', confidence: 55, outcome: 'pending' },
    { slug: 'HS2-W8M4ND', completed_at: '2026-09-30T09:00:00Z', outcome: 'pending' },
  ],
};

const STATE_CHOICES: SelectChoice[] = [
  { value: 'ready', label: 'Report' },
  { value: 'empty', label: 'No completions yet' },
  { value: 'loading', label: 'Loading' },
  { value: 'error', label: 'Error' },
];

export const confidenceCalibrationSettings = { state: signal<string>('ready') };

export function confidenceCalibrationDemoState(name: string): ConfidenceCalibrationState {
  if (name === 'loading') return { status: 'loading' };
  if (name === 'error') return { status: 'error', message: 'The Hot Sheet server could not be reached.' };
  if (name === 'empty')
    return { status: 'ready', report: { ...CALIBRATION_FIXTURE, completions: 0, scored: 0, events: [] } };
  return { status: 'ready', report: CALIBRATION_FIXTURE };
}

export function resetConfidenceCalibrationDemo(root?: ParentNode): void {
  confidenceCalibrationSettings.state.value = 'ready';
  if (root)
    syncSettingsControls(root, 'confidence-calibration', {
      values: { state: confidenceCalibrationSettings.state.value },
    });
}

export function ConfidenceCalibrationDemo() {
  return (
    <section class="component-stage" aria-label="ConfidenceCalibration demo">
      <div class="component-stage__canvas">
        {ConfidenceCalibration({ state: confidenceCalibrationDemoState(confidenceCalibrationSettings.state.value) })}
      </div>
      <p class="component-stage__guidance">
        Shown in a project's statistics view. Reopen rate counts completions with a known outcome; the bar appears once
        a band has one.
      </p>
    </section>
  );
}

export function ConfidenceCalibrationSettings() {
  return (
    <form class="settings-form" data-settings="confidence-calibration">
      <Select name="state" label="State" value={confidenceCalibrationSettings.state.value} choices={STATE_CHOICES} />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}
