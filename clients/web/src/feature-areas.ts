import { signal } from 'kerfjs';

import { isRecord } from './json-value';
import { type ResponseParser, ServerResponseShapeError } from './server-response-parsers';

/**
 * Feature areas (HS2-3GJZGG, docs/06-clients.md §6.12): the non-core parts of Hot Sheet a person can
 * turn on or off in App Settings › Features. Experimental areas are off by default; optional areas
 * are on by default so a person can turn them off for a quieter workspace.
 *
 * The choices are machine-wide server settings (`GET`/`PUT /feature-settings`, or
 * `hotsheet-cli settings set features.<id> on|off|default`) applied to every client of the host. A
 * feature that is off has no buttons, view-switcher choices, menu items, shortcuts, or settings
 * controls; its data is kept, work already running continues, and the CLI and MCP are unaffected.
 *
 * This is the client half of the registry. `crates/hotsheet-ticketing/src/features.rs` lists the
 * same ids, groups and defaults, and `feature-areas-policy.test.ts` pins them together and fails
 * when an entry point is neither core nor tagged with a registered feature (`data-feature`).
 */
export type FeatureId =
  | 'ai-chat'
  | 'drive'
  | 'project-stats'
  | 'cross-project-stats'
  | 'auto-grouping'
  | 'note-repair'
  | 'ai-recommendations'
  | 'code-review'
  | 'workspace-grid'
  | 'workers-view'
  | 'commands'
  | 'repository-status'
  | 'live-activity'
  | 'ticket-groups'
  | 'ticket-timeline'
  | 'confidence'
  | 'annotations'
  | 'not-working'
  | 'announcer'
  | 'standby';

/** Where the Features page lists an area. Experimental areas default off; the rest default on. */
export type FeatureGroup = 'experimental' | 'workspace' | 'tickets' | 'ambient';

export interface FeatureArea {
  id: FeatureId;
  group: FeatureGroup;
  defaultOn: boolean;
  label: string;
  /** What the checkbox turns on or off, in the words of the entry points it hides. */
  hint: string;
}

const experimental = (id: FeatureId, label: string, hint: string): FeatureArea => ({
  id,
  group: 'experimental',
  defaultOn: false,
  label,
  hint,
});
const optional = (group: Exclude<FeatureGroup, 'experimental'>, id: FeatureId, label: string, hint: string) =>
  ({ id, group, defaultOn: true, label, hint }) satisfies FeatureArea;

/** Every feature area, in the order the Features page lists them. */
export const FEATURE_AREAS: readonly FeatureArea[] = [
  experimental(
    'ai-chat',
    'AI chat',
    'The chat button in the project sidebar and new AI chats in the terminal drawer. Open chats stay.',
  ),
  experimental('drive', 'Drive with AI', 'The Drive button and its options at the foot of the project sidebar.'),
  experimental(
    'project-stats',
    'Project statistics',
    'The statistics page that opens from the completion chart. The chart itself stays.',
  ),
  experimental(
    'cross-project-stats',
    'Cross-project stats',
    'The Stats button in the project strip and its shortcut, with Right Now, Overnight, and This Mac.',
  ),
  experimental(
    'auto-grouping',
    'Automatic ticket grouping',
    'An AI turn picks or names a group for each new ticket. Off, no turn runs and the project setting is kept.',
  ),
  experimental('note-repair', 'Note repair', 'AI-proposed fixes to a note, reviewed before they are saved.'),
  experimental(
    'ai-recommendations',
    'AI recommendations',
    'The toolbar action that turns ticket feedback into proposed guidance changes.',
  ),
  experimental('code-review', 'Code review tab', 'The Code Review tab in the ticket inspector.'),
  optional(
    'workspace',
    'workspace-grid',
    'Workspace Grid',
    'Every project’s terminals and AI sessions on one page, from the project strip.',
  ),
  optional('workspace', 'workers-view', 'Workers view', 'The Workers choice in the view switcher.'),
  optional(
    'workspace',
    'commands',
    'Commands',
    'Command buttons in the project sidebar and Project Settings › Commands.',
  ),
  optional(
    'workspace',
    'repository-status',
    'Repository status',
    'The branch summary in the project sidebar and its changes popover.',
  ),
  optional('workspace', 'live-activity', 'Live activity', 'The Live section in the project sidebar.'),
  optional(
    'tickets',
    'ticket-groups',
    'Ticket groups and saved views',
    'Group sections and the Group menu, Project Settings › Ticket groups, and custom views. Off, grouped tickets show ungrouped.',
  ),
  optional('tickets', 'ticket-timeline', 'Ticket timeline', 'The Timeline tab in the ticket inspector.'),
  optional(
    'tickets',
    'confidence',
    'Completion confidence',
    'Confidence badges on completed tickets and the calibration table.',
  ),
  optional('tickets', 'annotations', 'Attachment annotations', 'Drawing and commenting on images in the gallery.'),
  optional('tickets', 'not-working', 'Not Working reports', 'The Not Working… action on tickets and its dialog.'),
  optional('ambient', 'announcer', 'Announcer', 'The activity reel and spoken announcements.'),
  optional('ambient', 'standby', 'Standby', 'The moon button, idle entry, and the away digest.'),
];

/** The Features page's section and sub-heading titles. */
export const FEATURE_GROUP_TITLES: Readonly<Record<FeatureGroup, string>> = {
  experimental: 'Experimental',
  workspace: 'Workspace',
  tickets: 'Tickets',
  ambient: 'Ambient',
};

const FEATURE_IDS: ReadonlySet<string> = new Set(FEATURE_AREAS.map((area) => area.id));

/** Whether `value` is a registered feature id. */
export const isFeatureId = (value: string): value is FeatureId => FEATURE_IDS.has(value);

/** The machine-wide feature settings (`GET`/`PUT /feature-settings`). */
export interface FeatureSettingsView {
  defaults: Readonly<Record<string, boolean>>;
  /** Explicit choices; `null` is "use the default". */
  configured: Readonly<Record<string, boolean | null>>;
  effective: Readonly<Record<string, boolean>>;
}

/** A `PUT /feature-settings` body: the choices to merge; `null` clears one to its default. */
export type FeatureSettingsChange = Partial<Record<FeatureId, boolean | null>>;

/** `GET`/`PUT /feature-settings`. Unknown ids (a newer server) are kept and ignored. */
export const parseFeatureSettings: ResponseParser<FeatureSettingsView> = (value) => {
  const what = 'feature settings';
  if (!isRecord(value)) throw new ServerResponseShapeError(what);
  for (const group of ['defaults', 'effective', 'configured'] as const) {
    const values = value[group];
    if (!isRecord(values)) throw new ServerResponseShapeError(what);
    for (const field of Object.values(values)) {
      if (typeof field === 'boolean') continue;
      if (group === 'configured' && field === null) continue;
      throw new ServerResponseShapeError(what);
    }
  }
  return value as unknown as FeatureSettingsView;
};

/**
 * The feature settings this client last read, or `undefined` before the first read. Rendering reads
 * it through `featureOn`, so a pushed change re-renders every gated entry point.
 */
export const featureSettings = signal<FeatureSettingsView | undefined>(undefined);

/** Forget the read settings (tests, and a client that lost its server); defaults apply again. */
export function resetFeatureSettings(): void {
  featureSettings.value = undefined;
}

/** A feature's registered default. */
export function featureDefault(id: FeatureId): boolean {
  return FEATURE_AREAS.find((area) => area.id === id)?.defaultOn ?? true;
}

/**
 * Whether a feature is on: the server's effective choice, else its registered default while the
 * settings are unread or the server predates the feature.
 */
export function featureOn(id: FeatureId, settings: FeatureSettingsView | undefined = featureSettings.value): boolean {
  const effective = settings?.effective[id];
  return typeof effective === 'boolean' ? effective : featureDefault(id);
}

/** The change that restores every feature to its default. */
export function restoreFeatureDefaults(): FeatureSettingsChange {
  return Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, null]));
}

/**
 * The attribute every gated entry point carries (`data-feature="<id>"`), so the policy test and a
 * reader can tell which feature an element belongs to.
 */
export const featureAttribute = (id: FeatureId) => ({ 'data-feature': id }) as const;

/**
 * The feature a whole-workspace shell mode belongs to: the Workspace Grid, cross-project stats, or one
 * project's statistics. A mode whose feature is off is ignored, like its shortcut (HS2-3GJZGG).
 */
export function shellModeFeature(mode: string, statsProjectId: string | undefined): FeatureId | undefined {
  if (mode === 'terminals') return 'workspace-grid';
  if (mode === 'stats') return statsProjectId === undefined ? 'cross-project-stats' : 'project-stats';
  return undefined;
}

/** Whether the shell may show `mode` now. */
export function shellModeEnabled(
  mode: string,
  statsProjectId: string | undefined,
  settings: FeatureSettingsView | undefined = featureSettings.value,
): boolean {
  const feature = shellModeFeature(mode, statsProjectId);
  return feature === undefined || featureOn(feature, settings);
}

/** The workspace views that belong to a feature area (HS2-3GJZGG); every other view is core. */
export const VIEW_MODE_FEATURES: Readonly<Partial<Record<string, FeatureId>>> = { workers: 'workers-view' };

/** Whether the view switcher offers `mode`. */
export function viewModeShown(
  mode: string,
  settings: FeatureSettingsView | undefined = featureSettings.value,
): boolean {
  const feature = VIEW_MODE_FEATURES[mode];
  return feature === undefined || featureOn(feature, settings);
}

/** A settings view with each feature at its default unless `choices` names it (demos and tests). */
export function featureSettingsView(choices: Partial<Record<FeatureId, boolean | null>> = {}): FeatureSettingsView {
  return {
    defaults: Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, area.defaultOn])),
    configured: Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, choices[area.id] ?? null])),
    effective: Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, choices[area.id] ?? area.defaultOn])),
  };
}

/** Every feature turned on: a test of a feature's own behavior starts here. */
export const allFeaturesOn = (): FeatureSettingsView =>
  featureSettingsView(Object.fromEntries(FEATURE_AREAS.map((area) => [area.id, true])));
