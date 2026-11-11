import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { closePopupMenu, type PopupMenuElement } from '@kerfjs/ui/popup-menu';
import { delegate, type Signal, signal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { Radio } from 'lucide';

import { ANNOUNCER_DOCK_REGION_ID, loadAnnouncerDockSize, saveAnnouncerDockSize } from '../announcer-layout';
import {
  announcerDigest,
  announcerItem,
  type AnnouncerReelItem,
  mergeAnnouncerReel,
  retainedAnnouncerReel,
} from '../announcer-reel';
import {
  ANNOUNCER_RATES,
  announcerVoiceChoices,
  loadAnnouncerSettings,
  saveAnnouncerSettings,
} from '../announcer-settings';
import { type ActivityEvent, Api } from '../api';
import { AnnouncerOverlay } from '../components/announcer-overlay';
import type { Project } from '../interactions/types';
import { regionCommitRoutes } from '../resizable-region-commits';

/** The open announcer in both of its places; AppShell shows the one that fits the rail (HS2-BS0XFK). */
export interface AnnouncerPlacements {
  docked: SafeHtml;
  corner: SafeHtml;
}

export interface AnnouncerController {
  trigger: () => SafeHtml;
  /** The open announcer's placements, or nothing while it is closed. */
  panel: () => AnnouncerPlacements | undefined;
  onActivity: (project: Project, storeId: string, activity: ActivityEvent) => void;
  dispose: () => void;
}

/** The real shell's retained digest and event-driven live playback (HS2-5PSQJQ). Web Speech is
 * the local voice provider; the server TTS boundary can be offered when configured. */
export function createAnnouncerController(projects: Signal<Project[]>): AnnouncerController {
  const open = signal(false);
  const expanded = signal(false);
  const live = signal(false);
  const speaking = signal(false);
  const audible = signal(true);
  const items = signal<AnnouncerReelItem[]>([]);
  const index = signal(0);
  const error = signal<string | undefined>();
  const currentDiff = signal<{ key: string; diff: string; truncated: boolean } | undefined>();
  const diffLoading = signal(false);
  const diffError = signal<string | undefined>();
  const providers = signal<string[]>([]);
  // HS2-PRCC6V: voice, speed, and the cleared-history mark are per-device settings.
  const stored = loadAnnouncerSettings(localStorage);
  const provider = signal(stored.provider);
  const voice = signal(stored.voice);
  const rate = signal(stored.rate);
  let clearedAt = stored.clearedAt;
  const browserVoices = signal<string[]>([]);
  const persistSettings = () => {
    saveAnnouncerSettings(localStorage, {
      provider: provider.peek(),
      voice: voice.peek(),
      rate: rate.peek(),
      clearedAt,
    });
  };
  const refreshVoices = () => {
    if (typeof speechSynthesis === 'undefined' || typeof speechSynthesis.getVoices !== 'function') return;
    browserVoices.value = announcerVoiceChoices(speechSynthesis.getVoices(), navigator.language, voice.peek());
  };
  /** Activity cleared from this device's reel stays out of it (HS2-PRCC6V). */
  const uncleared = (reel: AnnouncerReelItem[]) =>
    clearedAt ? reel.filter((item) => item.event.ts > clearedAt) : reel;
  const dockSize = signal(loadAnnouncerDockSize(localStorage));
  const unrouteDock = regionCommitRoutes.route(ANNOUNCER_DOCK_REGION_ID, (size) => {
    dockSize.value = saveAnnouncerDockSize(localStorage, size);
  });
  let requestGeneration = 0;
  let diffGeneration = 0;
  let speechGeneration = 0;
  let currentUtterance: SpeechSynthesisUtterance | undefined;
  let currentAudio: HTMLAudioElement | undefined;
  let currentUrl: string | undefined;

  function cancelSpeech() {
    speechGeneration += 1;
    if (currentUtterance && typeof speechSynthesis !== 'undefined') speechSynthesis.cancel();
    currentUtterance = undefined;
    currentAudio?.pause();
    currentAudio = undefined;
    if (currentUrl) URL.revokeObjectURL(currentUrl);
    currentUrl = undefined;
    speaking.value = false;
  }

  function clearDiff() {
    diffGeneration += 1;
    currentDiff.value = undefined;
    diffLoading.value = false;
    diffError.value = undefined;
  }

  async function loadCurrentDiff() {
    const item = items.peek().at(index.peek());
    if (!item?.change || diffLoading.peek()) return;
    const project = projects.peek().find((entry) => entry.id === item.projectId);
    if (!project) {
      diffError.value = 'Project is no longer open.';
      return;
    }
    const generation = ++diffGeneration;
    currentDiff.value = undefined;
    diffError.value = undefined;
    diffLoading.value = true;
    try {
      const diff = await new Api(project.apiPath).checkoutCurrentDiff(item.projectId, item.change.path);
      if (generation === diffGeneration && open.peek()) currentDiff.value = { key: item.key, ...diff };
    } catch {
      if (generation === diffGeneration && open.peek()) diffError.value = 'Current diff could not be loaded.';
    } finally {
      if (generation === diffGeneration) diffLoading.value = false;
    }
  }

  async function narrateCurrent() {
    const item = items.peek().at(index.peek());
    if (!item || !audible.peek()) return;
    cancelSpeech();
    const generation = speechGeneration;
    const onEnd = () => {
      if (generation !== speechGeneration) return;
      currentUtterance = undefined;
      currentAudio = undefined;
      if (currentUrl) URL.revokeObjectURL(currentUrl);
      currentUrl = undefined;
      speaking.value = false;
      if (live.peek() && index.peek() < items.peek().length - 1) {
        index.value += 1;
        void narrateCurrent();
      }
    };
    error.value = undefined;
    if (provider.peek() !== 'browser') {
      try {
        const project = projects.peek().find((entry) => entry.id === item.projectId);
        if (!project) throw new Error('Project is no longer open.');
        const blob = await new Api(project.apiPath).synthesizeSpeech(
          item.spoken,
          provider.peek(),
          voice.peek() || undefined,
        );
        if (generation !== speechGeneration) return;
        currentUrl = URL.createObjectURL(blob);
        const audio = new Audio(currentUrl);
        audio.playbackRate = rate.peek();
        currentAudio = audio;
        audio.onended = onEnd;
        audio.onerror = () => {
          if (generation === speechGeneration) {
            speaking.value = false;
            error.value = 'Narration could not be played.';
          }
        };
        speaking.value = true;
        await audio.play();
      } catch {
        if (generation === speechGeneration) {
          speaking.value = false;
          error.value = 'Server voice is unavailable.';
        }
      }
      return;
    }
    if (!('speechSynthesis' in window)) {
      error.value = 'This browser has no local speech voice.';
      return;
    }
    // Speech says the item's summary, never its literal command (HS2-R5Q2GY).
    const utterance = new SpeechSynthesisUtterance(item.spoken);
    utterance.rate = rate.peek();
    if (voice.peek()) {
      const selected = speechSynthesis.getVoices().find((candidate) => candidate.name === voice.peek());
      if (selected) utterance.voice = selected;
    }
    utterance.onstart = () => {
      if (generation === speechGeneration) speaking.value = true;
    };
    utterance.onerror = () => {
      if (generation === speechGeneration) {
        speaking.value = false;
        error.value = 'Narration could not be played.';
      }
    };
    utterance.onend = onEnd;
    currentUtterance = utterance;
    speechSynthesis.speak(utterance);
  }

  async function loadDigest() {
    const generation = ++requestGeneration;
    const primary = projects.peek().at(0);
    if (primary) {
      void new Api(primary.apiPath).ttsProviders().then(
        (available) => {
          if (generation === requestGeneration) providers.value = available;
        },
        () => {
          if (generation === requestGeneration) providers.value = [];
        },
      );
    }
    const results = await Promise.allSettled(
      projects.peek().map(async (project) => ({
        id: project.id,
        name: project.name,
        events: await new Api(project.apiPath).checkoutActivity(project.id, { limit: 60 }),
      })),
    );
    if (generation !== requestGeneration || !open.peek()) return;
    const loaded = results.flatMap((result) => (result.status === 'fulfilled' ? [result.value] : []));
    if (loaded.length === 0 && results.length > 0) error.value = 'Activity could not be loaded.';
    const selectedKey = items.peek().at(index.peek())?.key;
    items.value = uncleared(mergeAnnouncerReel(items.peek(), retainedAnnouncerReel(loaded)));
    if (speaking.peek() && selectedKey) {
      const selection = items.peek().findIndex((item) => item.key === selectedKey);
      if (selection >= 0) {
        index.value = selection;
        return;
      }
    }
    if (live.peek()) {
      clearDiff();
      index.value = Math.max(0, items.peek().length - 1);
      return;
    }
    const digest = announcerDigest(items.peek());
    clearDiff();
    index.value = digest.length ? items.peek().findIndex((item) => item.key === digest[0].key) : 0;
  }

  function onActivity(project: Project, storeId: string, activity: ActivityEvent) {
    const item = announcerItem(project.id, project.name, storeId, activity);
    if (clearedAt && item.event.ts <= clearedAt) return;
    const isNew = !items.peek().some((entry) => entry.key === item.key);
    items.value = mergeAnnouncerReel(items.peek(), [item]);
    if (live.peek() && open.peek() && isNew && !speaking.peek()) {
      clearDiff();
      index.value = items.peek().findIndex((entry) => entry.key === item.key);
      void narrateCurrent();
    }
  }

  const unbind = delegate(document.body, 'click', '[data-announcer-action]', (_event, target) => {
    const element = target as HTMLElement;
    const requestedAction = element.dataset.announcerAction;
    if (!requestedAction) return;
    switch (requestedAction) {
      case 'open':
        clearDiff();
        open.value = !open.peek();
        if (open.peek()) void loadDigest();
        else {
          live.value = false;
          cancelSpeech();
        }
        break;
      case 'expand':
        expanded.value = !expanded.peek();
        break;
      case 'play':
        if (speaking.peek()) cancelSpeech();
        else void narrateCurrent();
        break;
      case 'previous':
        clearDiff();
        cancelSpeech();
        index.value = Math.max(0, index.peek() - 1);
        break;
      case 'next':
        clearDiff();
        cancelSpeech();
        index.value = Math.min(items.peek().length - 1, index.peek() + 1);
        break;
      case 'live':
        clearDiff();
        live.value = !live.peek();
        if (live.peek() && items.peek().length) index.value = items.peek().length - 1;
        break;
      case 'audio':
        audible.value = !audible.peek();
        if (!audible.peek()) cancelSpeech();
        break;
      case 'current-diff':
        void loadCurrentDiff();
        break;
      default:
        break;
    }
  });
  function applySetting(setting: string, value: string) {
    switch (setting) {
      case 'provider':
        if (value !== provider.peek() && (value === 'browser' || providers.peek().includes(value))) {
          cancelSpeech();
          provider.value = value;
          // A voice name belongs to its engine; the new engine starts at its default voice.
          voice.value = '';
        }
        break;
      case 'voice':
        cancelSpeech();
        voice.value = value;
        break;
      case 'rate': {
        const chosen = ANNOUNCER_RATES.find((choice) => String(choice) === value);
        if (chosen !== undefined) rate.value = chosen;
        break;
      }
      case 'clear-history': {
        cancelSpeech();
        clearDiff();
        const latest = items.peek().reduce((max, item) => (item.event.ts > max ? item.event.ts : max), clearedAt);
        clearedAt = latest;
        items.value = [];
        index.value = 0;
        break;
      }
      default:
        return;
    }
    persistSettings();
  }
  const unbindSettings = delegate(document.body, 'kui-menu-select', '[data-announcer-settings]', (event) => {
    const item = (event as CustomEvent<{ item: HTMLElement }>).detail.item;
    // A host update can replace the menu before Kerf's post-event close runs (HS2-2055AY).
    closePopupMenu(event.target as PopupMenuElement);
    const { announcerSetting, announcerValue } = item.dataset;
    if (announcerSetting) applySetting(announcerSetting, announcerValue ?? '');
  });
  // Some engines (and test doubles) expose speech without events; their voice list is read once.
  const voicesSource =
    typeof speechSynthesis !== 'undefined' && typeof speechSynthesis.addEventListener === 'function'
      ? speechSynthesis
      : undefined;
  voicesSource?.addEventListener('voiceschanged', refreshVoices);
  refreshVoices();
  return {
    trigger: () => (
      <button
        type="button"
        aria-label="Announcer"
        title="Announcer"
        aria-pressed={String(open.value)}
        data-announcer-action="open"
      >
        <LucideIcon icon={Radio} name="radio" />
      </button>
    ),
    panel: () => {
      if (!open.value) return undefined;
      const overlay = (presentation: 'docked' | 'corner') => (
        <AnnouncerOverlay
          open
          presentation={presentation}
          dockSize={dockSize.value}
          expanded={expanded.value}
          live={live.value}
          speaking={speaking.value}
          audible={audible.value}
          items={items.value}
          index={index.value}
          error={error.value}
          currentDiff={currentDiff.value}
          diffLoading={diffLoading.value}
          diffError={diffError.value}
          providers={providers.value}
          provider={provider.value}
          voice={voice.value}
          voices={browserVoices.value}
          rate={rate.value}
        />
      );
      return { docked: overlay('docked'), corner: overlay('corner') };
    },
    onActivity,
    dispose: () => {
      unrouteDock();
      unbind();
      unbindSettings();
      voicesSource?.removeEventListener('voiceschanged', refreshVoices);
      cancelSpeech();
      clearDiff();
    },
  };
}
