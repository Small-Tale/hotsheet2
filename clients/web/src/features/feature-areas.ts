import { signal } from 'kerfjs';

import type { ChangeEvent } from '../api';
import {
  featureSettings,
  type FeatureSettingsChange,
  type FeatureSettingsView,
  isFeatureId,
  restoreFeatureDefaults,
} from '../feature-areas';

/** The server calls App Settings › Features makes (HS2-3GJZGG). */
export interface FeatureAreasApi {
  featureSettings(): Promise<FeatureSettingsView>;
  saveFeatureSettings(change: FeatureSettingsChange): Promise<FeatureSettingsView>;
}

export interface FeatureAreasControllerDependencies {
  /** The API of the server this client talks to, once a project is open. */
  api: () => FeatureAreasApi | undefined;
}

/** The change event kind that announces a feature-settings change. */
export const FEATURE_SETTINGS_EVENT = 'feature_settings';

const message = (reason: unknown) => (reason instanceof Error ? reason.message : String(reason));

/**
 * Keeps the shared `featureSettings` signal current (HS2-3GJZGG). It reads once when a project opens,
 * re-reads once per new `feature_settings` change event and on each stream resync, and never on a
 * timer. A checkbox saves immediately; a failed save shows its reason and re-reads the truth.
 */
export function createFeatureAreasController(dependencies: FeatureAreasControllerDependencies) {
  const settingsMessage = signal('');
  let generation = 0;

  async function refresh(keepMessage = false) {
    const api = dependencies.api();
    if (!api) return;
    const request = ++generation;
    try {
      const value = await api.featureSettings();
      if (request !== generation) return;
      featureSettings.value = value;
      if (!keepMessage) settingsMessage.value = '';
    } catch (reason) {
      // An older server has no feature route: every feature keeps its registered default.
      if (request === generation) settingsMessage.value = message(reason);
    }
  }

  let read: Promise<void> | undefined,
    readAgain = false,
    /** The newest event cursor acted on; reset on resync because a restarted server restarts its cursors. */
    seenEvent: number | undefined;

  /** Re-read now, or once more after the read already in flight. */
  function reread(): Promise<void> {
    if (read) {
      readAgain = true;
      return read;
    }
    const again = () => readAgain;
    read = (async () => {
      for (;;) {
        readAgain = false;
        await refresh();
        if (!again()) return;
      }
    })().finally(() => {
      read = undefined;
    });
    return read;
  }

  /** Feed change events from any project stream; the same event seen through several streams reads once. */
  function applyEvents(events: readonly ChangeEvent[]): Promise<void> | undefined {
    let fresh = false;
    for (const event of events) {
      if (event.kind !== FEATURE_SETTINGS_EVENT) continue;
      if (event.cursor === undefined || seenEvent === undefined || event.cursor > seenEvent) fresh = true;
      if (event.cursor !== undefined) seenEvent = Math.max(seenEvent ?? event.cursor, event.cursor);
    }
    return fresh ? reread() : undefined;
  }

  /** A change stream (re)started or overflowed: an event may have been missed. */
  function resync(): Promise<void> {
    seenEvent = undefined;
    return reread();
  }

  async function save(change: FeatureSettingsChange) {
    const api = dependencies.api();
    if (!api) return;
    const request = ++generation;
    settingsMessage.value = '';
    try {
      const value = await api.saveFeatureSettings(change);
      if (request !== generation) return;
      featureSettings.value = value;
    } catch (reason) {
      if (request !== generation) return;
      settingsMessage.value = message(reason);
      await refresh(true);
    }
  }

  /** A Features checkbox changed. */
  function setFeature(id: string, on: boolean): Promise<void> | undefined {
    if (!isFeatureId(id)) return undefined;
    return save({ [id]: on });
  }

  return {
    settingsMessage,
    refresh,
    applyEvents,
    resync,
    setFeature,
    restoreDefaults: () => save(restoreFeatureDefaults()),
  };
}

export type FeatureAreasController = ReturnType<typeof createFeatureAreasController>;
