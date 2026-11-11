import type { PopupMenuEntry } from '@kerfjs/ui/popup-menu';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

/** The announcer's per-device narration settings (HS2-PRCC6V). */
export interface AnnouncerSettings {
  /** `browser` (Web Speech) or a server provider id such as `macos-say`. */
  provider: string;
  /** A voice name; empty for the engine's default voice. */
  voice: string;
  /** Speech rate, one of {@link ANNOUNCER_RATES}. */
  rate: number;
  /** Activity recorded at or before this timestamp stays out of the reel ("Clear history"). */
  clearedAt: string;
}

export const ANNOUNCER_RATES = [0.75, 1, 1.25, 1.5, 2] as const;
export const DEFAULT_ANNOUNCER_SETTINGS: AnnouncerSettings = { provider: 'browser', voice: '', rate: 1, clearedAt: '' };
/** Voices listed in the menu at most; the selected voice is always kept. */
export const ANNOUNCER_VOICE_LIMIT = 24;

const SETTINGS_KEY = 'hotsheet.announcer.settings';

const text = (value: unknown, fallback: string) => (typeof value === 'string' ? value : fallback);

/** The device's stored settings; unknown or malformed fields fall back to their defaults. */
export function loadAnnouncerSettings(storage: Pick<Storage, 'getItem'>): AnnouncerSettings {
  let parsed: unknown;
  try {
    parsed = JSON.parse(storage.getItem(SETTINGS_KEY) ?? 'null');
  } catch {
    return { ...DEFAULT_ANNOUNCER_SETTINGS };
  }
  if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return { ...DEFAULT_ANNOUNCER_SETTINGS };
  const stored = parsed as Record<string, unknown>;
  const rate = ANNOUNCER_RATES.find((choice) => choice === stored.rate) ?? DEFAULT_ANNOUNCER_SETTINGS.rate;
  return {
    provider: text(stored.provider, DEFAULT_ANNOUNCER_SETTINGS.provider).trim() || DEFAULT_ANNOUNCER_SETTINGS.provider,
    voice: text(stored.voice, '').trim(),
    rate,
    clearedAt: text(stored.clearedAt, ''),
  };
}

export function saveAnnouncerSettings(storage: Pick<Storage, 'setItem'>, settings: AnnouncerSettings): void {
  try {
    storage.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // A full or blocked store keeps the settings for this session.
  }
}

/** A rate's menu label: "Normal" for 1, otherwise its multiplier. */
export const announcerRateLabel = (rate: number) => (rate === 1 ? 'Normal' : `${rate}×`);

/**
 * The browser voices offered for the page's language, by name, capped at {@link ANNOUNCER_VOICE_LIMIT}.
 * The selected voice stays listed even when it falls outside the language or the cap.
 */
export function announcerVoiceChoices(
  voices: readonly { name: string; lang: string }[],
  language: string,
  selected: string,
  limit = ANNOUNCER_VOICE_LIMIT,
): string[] {
  const prefix = language.toLowerCase().split('-')[0] ?? '';
  const names = [
    ...new Set(
      voices
        .filter((voice) => !prefix || voice.lang.toLowerCase().split(/[-_]/)[0] === prefix)
        .map((voice) => voice.name),
    ),
  ]
    .sort((left, right) => left.localeCompare(right))
    .slice(0, Math.max(0, limit));
  return selected && !names.includes(selected) ? [selected, ...names] : names;
}

const providerLabel = (id: string) =>
  id === 'browser' ? 'Browser voice' : id === 'macos-say' ? 'Mac voice on server' : id;

/** Menu entries for the announcer's settings button. Each choice carries its action and value as
 * `data-announcer-setting` and `data-announcer-value` attributes for the menu's select handler. */
export function announcerSettingsMenu({
  providers,
  provider,
  voices,
  voice,
  rate,
  hasHistory,
  icons,
}: {
  providers: readonly string[];
  provider: string;
  /** Browser voice names; ignored for a server provider, which offers only its default voice. */
  voices: readonly string[];
  voice: string;
  rate: number;
  hasHistory: boolean;
  icons: { voice: SafeHtml; speed: SafeHtml; clear: SafeHtml };
}): PopupMenuEntry[] {
  const choice = (setting: string, value: string, label: string, checked: boolean): PopupMenuEntry => ({
    label,
    checked,
    attributes: { 'data-announcer-setting': setting, 'data-announcer-value': value },
  });
  const engines = ['browser', ...providers.filter((id) => id !== 'browser')];
  const voiceNames = provider === 'browser' ? voices : [];
  return [
    {
      label: 'Voice',
      icon: icons.voice,
      submenu: [
        { kind: 'heading', label: 'Engine' },
        ...engines.map((id) => choice('provider', id, providerLabel(id), id === provider)),
        { kind: 'divider' },
        { kind: 'heading', label: 'Voice' },
        choice('voice', '', 'Default voice', !voice),
        ...voiceNames.map((name) => choice('voice', name, name, name === voice)),
      ],
    },
    {
      label: 'Speed',
      icon: icons.speed,
      submenu: ANNOUNCER_RATES.map((choiceRate) =>
        choice('rate', String(choiceRate), announcerRateLabel(choiceRate), choiceRate === rate),
      ),
    },
    { kind: 'divider' },
    {
      label: 'Clear history',
      icon: icons.clear,
      disabled: !hasHistory,
      disabledReason: hasHistory ? undefined : 'No activity to clear',
      attributes: { 'data-announcer-setting': 'clear-history', 'data-announcer-value': '' },
    },
  ];
}
