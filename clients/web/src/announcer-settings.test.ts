import type { PopupMenuEntry, PopupMenuItem } from '@kerfjs/ui/popup-menu';
import { describe, expect, it } from 'vitest';

import {
  ANNOUNCER_VOICE_LIMIT,
  announcerRateLabel,
  announcerSettingsMenu,
  announcerVoiceChoices,
  DEFAULT_ANNOUNCER_SETTINGS,
  loadAnnouncerSettings,
  saveAnnouncerSettings,
} from './announcer-settings';

const memory = (initial?: string) => {
  const values = new Map<string, string>(initial === undefined ? [] : [['hotsheet.announcer.settings', initial]]);
  return {
    values,
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  };
};

const icons = { voice: 'V' as never, speed: 'S' as never, clear: 'C' as never };
const items = (entries: readonly PopupMenuEntry[]) =>
  entries.filter((entry): entry is PopupMenuItem => entry.kind !== 'heading' && entry.kind !== 'divider');
const checked = (entries: readonly PopupMenuEntry[]) =>
  items(entries)
    .filter((entry) => entry.checked)
    .map((entry) => entry.label);

describe('announcer settings persistence (HS2-PRCC6V)', () => {
  it('defaults without stored, malformed, or out-of-range settings', () => {
    for (const stored of [undefined, '', '{', '[]', 'null', '7'])
      expect(loadAnnouncerSettings(memory(stored))).toEqual(DEFAULT_ANNOUNCER_SETTINGS);
    expect(loadAnnouncerSettings(memory(JSON.stringify({ provider: ' ', voice: 3, rate: 1.1 })))).toEqual(
      DEFAULT_ANNOUNCER_SETTINGS,
    );
    expect(
      loadAnnouncerSettings({
        getItem: () => {
          throw new Error('blocked');
        },
      }),
    ).toEqual(DEFAULT_ANNOUNCER_SETTINGS);
  });

  it('round-trips settings and survives a refused write', () => {
    const storage = memory();
    const settings = { provider: 'macos-say', voice: 'Samantha', rate: 1.5, clearedAt: '2026-11-01T01:00:00Z' };
    saveAnnouncerSettings(storage, settings);
    expect(loadAnnouncerSettings(storage)).toEqual(settings);
    saveAnnouncerSettings(storage, { ...settings, rate: 0.75, voice: '' });
    expect(loadAnnouncerSettings(storage)).toEqual({ ...settings, rate: 0.75, voice: '' });
    expect(() => {
      saveAnnouncerSettings(
        {
          setItem: () => {
            throw new Error('quota');
          },
        },
        settings,
      );
    }).not.toThrow();
  });
});

describe('announcer voice choices (HS2-PRCC6V)', () => {
  const voices = [
    { name: 'Zed', lang: 'en-GB' },
    { name: 'Amélie', lang: 'fr-CA' },
    { name: 'Alex', lang: 'en_US' },
    { name: 'Alex', lang: 'en-US' },
  ];

  it('lists the page language by name, once each, and keeps the selected voice', () => {
    expect(announcerVoiceChoices(voices, 'en-US', '')).toEqual(['Alex', 'Zed']);
    expect(announcerVoiceChoices(voices, 'en-US', 'Amélie')).toEqual(['Amélie', 'Alex', 'Zed']);
    expect(announcerVoiceChoices(voices, '', '')).toEqual(['Alex', 'Amélie', 'Zed']);
    expect(announcerVoiceChoices([], 'en', '')).toEqual([]);
  });

  it('caps a long list', () => {
    const many = Array.from({ length: 40 }, (_, at) => ({ name: `Voice ${String(at).padStart(2, '0')}`, lang: 'en' }));
    expect(announcerVoiceChoices(many, 'en', '')).toHaveLength(ANNOUNCER_VOICE_LIMIT);
    expect(announcerVoiceChoices(many, 'en', 'Voice 39')).toHaveLength(ANNOUNCER_VOICE_LIMIT + 1);
  });
});

describe('announcer settings menu (HS2-PRCC6V)', () => {
  const menu = (overrides: Partial<Parameters<typeof announcerSettingsMenu>[0]> = {}) =>
    announcerSettingsMenu({
      providers: ['macos-say'],
      provider: 'browser',
      voices: ['Alex', 'Zed'],
      voice: 'Zed',
      rate: 1.25,
      hasHistory: true,
      icons,
      ...overrides,
    });

  it('offers voice, speed, and clear history, each with an icon', () => {
    const top = items(menu());
    expect(top.map((entry) => entry.label)).toEqual(['Voice', 'Speed', 'Clear history']);
    expect(top.every((entry) => entry.icon)).toBe(true);
  });

  it('checks the current engine, voice, and speed', () => {
    const [voice, speed] = items(menu());
    expect(checked(voice.submenu ?? [])).toEqual(['Browser voice', 'Zed']);
    expect(items(voice.submenu ?? []).map((entry) => entry.label)).toEqual([
      'Browser voice',
      'Mac voice on server',
      'Default voice',
      'Alex',
      'Zed',
    ]);
    expect(checked(speed.submenu ?? [])).toEqual([announcerRateLabel(1.25)]);
    expect(items(speed.submenu ?? []).map((entry) => entry.label)).toEqual(['0.75×', 'Normal', '1.25×', '1.5×', '2×']);
    const zed = items(voice.submenu ?? []).find((entry) => entry.label === 'Zed');
    expect(zed?.attributes).toEqual({ 'data-announcer-setting': 'voice', 'data-announcer-value': 'Zed' });
  });

  it('offers only the default voice for a server engine and disables an empty clear', () => {
    const [voice, , clear] = items(menu({ provider: 'macos-say', voice: '', hasHistory: false }));
    expect(checked(voice.submenu ?? [])).toEqual(['Mac voice on server', 'Default voice']);
    expect(items(voice.submenu ?? []).some((entry) => entry.label === 'Alex')).toBe(false);
    expect(clear).toMatchObject({ disabled: true, disabledReason: 'No activity to clear' });
  });
});
