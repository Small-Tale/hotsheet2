import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { closePopupMenu, type PopupMenuElement } from '@kerfjs/ui/popup-menu';
import { wirePinnablePopover } from '@kerfjs/ui/surface-scaffold';
import { delegate, type Signal, signal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { Radio } from 'lucide';

import { type AnnouncerEntry, announcerSpeaks, editAnnouncerReel, nextSettleAt } from '../announcer-editorial';
import {
  ANNOUNCER_DOCK_REGION_ID,
  ANNOUNCER_FLOATING_ID,
  loadAnnouncerDetached,
  loadAnnouncerDockSize,
  loadAnnouncerFloatingPlacement,
  saveAnnouncerDetached,
  saveAnnouncerDockSize,
  saveAnnouncerFloatingPlacement,
} from '../announcer-layout';
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
import { announcerUtterance } from '../announcer-utterance';
import { type ActivityEvent, Api, type TtsVoice } from '../api';
import { AnnouncerOverlay } from '../components/announcer-overlay';
import type { Project } from '../interactions/types';
import { regionCommitRoutes } from '../resizable-region-commits';

/** The open announcer's places. Attached, it has both `docked` and `corner` and AppShell shows the one
 * that fits the rail (HS2-BS0XFK); detached, it has only its `floating` surface (HS2-51X25Z). */
export type AnnouncerPlacements =
  { docked: SafeHtml; corner: SafeHtml; floating?: never } | { floating: SafeHtml; docked?: never; corner?: never };

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
  /** Every retained and live event; the announcer shows and speaks its edited `entries` (HS2-RKYCTP). */
  const items = signal<AnnouncerReelItem[]>([]);
  const entries = signal<AnnouncerEntry[]>([]);
  const index = signal(0);
  /** When this device received each live event, so a routine group's window runs on one clock. */
  const receivedAt = new Map<string, number>();
  /** Reel items live narration has said, or that were already settled when it started. Items, not
   * entry keys: when the bounded reel drops a group's first step, the group's key moves on. */
  const heard = new Set<string>();
  const isHeard = (entry: AnnouncerEntry) => entry.members.some((member) => heard.has(member.key));
  const hear = (entry: AnnouncerEntry) => {
    for (const member of entry.members) heard.add(member.key);
  };
  let settleTimer: ReturnType<typeof setTimeout> | undefined;
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
  // HS2-CEZVSE: the selected server engine's voices, read when the announcer opens or the
  // engine changes. An engine that cannot list voices keeps only its default voice.
  const serverVoices = signal<TtsVoice[]>([]);
  let serverVoicesGeneration = 0;
  const loadServerVoices = () => {
    const generation = ++serverVoicesGeneration;
    serverVoices.value = [];
    const engine = provider.peek();
    const primary = projects.peek().at(0);
    if (engine === 'browser' || !primary) return;
    void new Api(primary.apiPath).ttsVoices(engine).then(
      (listed) => {
        if (generation === serverVoicesGeneration) serverVoices.value = listed;
      },
      () => undefined,
    );
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
  // HS2-51X25Z: popped out of the sidebar, the announcer is a persistent floating surface whose detached
  // state, position, and size this device remembers.
  const detached = signal(loadAnnouncerDetached(localStorage));
  const setDetached = (next: boolean) => {
    detached.value = next;
    saveAnnouncerDetached(localStorage, next);
  };
  let floating: { element: HTMLElement; dispose: () => void } | undefined;
  /** Wire each rendered floating surface once: Kerf moves, resizes, and clamps it, and this device stores
   * what it reports. */
  const syncFloating = () => {
    const element = document.getElementById(ANNOUNCER_FLOATING_ID);
    if (floating && floating.element !== element) {
      floating.dispose();
      floating = undefined;
    }
    if (!element) return;
    if (!floating) {
      const dispose = wirePinnablePopover(element, {
        placement: loadAnnouncerFloatingPlacement(localStorage),
        onChange: (change) => {
          if (change.placement) saveAnnouncerFloatingPlacement(localStorage, change.placement);
        },
      });
      floating = { element, dispose };
    }
    // The surface is persistent: only app state removes it. Moving its node, even within the DOM, closes
    // an open popover, and a re-render that reorders the shell's unkeyed slots keeps the same node, so
    // reopen it after every mutation rather than only when it is first wired (HS2-F880FN).
    if (element.isConnected && element.hasAttribute('popover') && !element.matches(':popover-open'))
      element.showPopover();
  };
  const floatingObserver = new MutationObserver(syncFloating);
  floatingObserver.observe(document.body, { childList: true, subtree: true });
  let requestGeneration = 0;
  let diffGeneration = 0;
  let speechGeneration = 0;
  let currentUtterance: SpeechSynthesisUtterance | undefined;
  let currentAudio: HTMLAudioElement | undefined;
  let currentUrl: string | undefined;
  /** Narration has started and not yet ended, including the gap before speech reports its start. */
  let narrating = false;
  /** The project the last utterance was about; a different one is named first (HS2-MR8GA4). Opening the
   * announcer forgets it, so the first utterance always names its project. */
  let lastSpokenProject: string | undefined;

  const editorialOptions = () => ({ now: Date.now(), receivedAt: (key: string) => receivedAt.get(key) });

  /** Edit the reel again, keeping the selected entry, then let live narration say what settled. */
  function refreshEntries() {
    const selectedKey = entries.peek().at(index.peek())?.key;
    entries.value = editAnnouncerReel(items.peek(), editorialOptions());
    // Forget items the bounded reel dropped; a group keeps every member it was heard with.
    const kept = new Set(items.peek().map((item) => item.key));
    for (const key of [...heard, ...receivedAt.keys()])
      if (!kept.has(key)) {
        heard.delete(key);
        receivedAt.delete(key);
      }
    const selection = selectedKey ? entries.peek().findIndex((entry) => entry.key === selectedKey) : -1;
    if (selection >= 0) index.value = selection;
    else index.value = Math.min(index.peek(), Math.max(0, entries.peek().length - 1));
    pumpLive();
    scheduleSettle();
  }

  /** While live, wake when the earliest open routine group settles; no timer otherwise. */
  function scheduleSettle() {
    if (settleTimer !== undefined) clearTimeout(settleTimer);
    settleTimer = undefined;
    if (!live.peek() || !open.peek()) return;
    const at = nextSettleAt(entries.peek(), editorialOptions());
    if (at === undefined) return;
    settleTimer = setTimeout(
      () => {
        settleTimer = undefined;
        refreshEntries();
      },
      Math.max(0, at - Date.now()),
    );
  }

  /** Entries settled now count as heard, so live narration starts from what happens next. */
  function markHeard() {
    for (const entry of entries.peek()) if (entry.settled) hear(entry);
  }

  /** Say the next settled entry live narration has not said, in reel order. */
  function pumpLive() {
    if (!live.peek() || !open.peek() || narrating || speaking.peek()) return;
    const list = entries.peek();
    let next = list.findIndex((entry) => entry.settled && !isHeard(entry));
    if (next < 0) return;
    clearDiff();
    if (!audible.peek()) {
      // Muted, live mode still follows the newest settled entry.
      markHeard();
      index.value = list.reduce((latest, entry, at) => (entry.settled ? at : latest), next);
      return;
    }
    // Routine steps and turn boundaries are shown as live mode passes them, but not said (HS2-G3HJZH).
    while (next >= 0 && !announcerSpeaks(list[next])) {
      hear(list[next]);
      index.value = next;
      next = list.findIndex((entry) => entry.settled && !isHeard(entry));
    }
    if (next < 0) return;
    hear(list[next]);
    index.value = next;
    void narrateCurrent();
  }

  function cancelSpeech() {
    speechGeneration += 1;
    narrating = false;
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
    const item = entries.peek().at(index.peek());
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
    const item = entries.peek().at(index.peek());
    if (!item || !audible.peek()) return;
    cancelSpeech();
    narrating = true;
    const generation = speechGeneration;
    const onEnd = () => {
      if (generation !== speechGeneration) return;
      narrating = false;
      currentUtterance = undefined;
      currentAudio = undefined;
      if (currentUrl) URL.revokeObjectURL(currentUrl);
      currentUrl = undefined;
      speaking.value = false;
      pumpLive();
    };
    error.value = undefined;
    const text = announcerUtterance(item, lastSpokenProject);
    lastSpokenProject = item.projectId;
    if (provider.peek() !== 'browser') {
      try {
        const project = projects.peek().find((entry) => entry.id === item.projectId);
        if (!project) throw new Error('Project is no longer open.');
        const blob = await new Api(project.apiPath).synthesizeSpeech(text, provider.peek(), voice.peek() || undefined);
        if (generation !== speechGeneration) return;
        currentUrl = URL.createObjectURL(blob);
        const audio = new Audio(currentUrl);
        audio.playbackRate = rate.peek();
        currentAudio = audio;
        audio.onended = onEnd;
        audio.onerror = () => {
          if (generation === speechGeneration) {
            narrating = false;
            speaking.value = false;
            error.value = 'Narration could not be played.';
          }
        };
        speaking.value = true;
        await audio.play();
      } catch {
        if (generation === speechGeneration) {
          narrating = false;
          speaking.value = false;
          error.value = 'Server voice is unavailable.';
        }
      }
      return;
    }
    if (!('speechSynthesis' in window)) {
      narrating = false;
      error.value = 'This browser has no local speech voice.';
      return;
    }
    // Speech says the item's summary, never its literal command (HS2-R5Q2GY).
    const utterance = new SpeechSynthesisUtterance(text);
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
        narrating = false;
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
    loadServerVoices();
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
    const selectedKey = entries.peek().at(index.peek())?.key;
    items.value = uncleared(mergeAnnouncerReel(items.peek(), retainedAnnouncerReel(loaded)));
    entries.value = editAnnouncerReel(items.peek(), editorialOptions());
    if (speaking.peek() && selectedKey) {
      const selection = entries.peek().findIndex((entry) => entry.key === selectedKey);
      if (selection >= 0) {
        index.value = selection;
        return;
      }
    }
    if (live.peek()) {
      clearDiff();
      // Retained history is not news: live narration continues from what happens next.
      markHeard();
      index.value = Math.max(0, entries.peek().length - 1);
      scheduleSettle();
      return;
    }
    const digest = announcerDigest(entries.peek());
    clearDiff();
    index.value = digest.length ? entries.peek().findIndex((entry) => entry.key === digest[0].key) : 0;
  }

  function onActivity(project: Project, storeId: string, activity: ActivityEvent) {
    const item = announcerItem(project.id, project.name, storeId, activity);
    if (clearedAt && item.event.ts <= clearedAt) return;
    if (items.peek().some((entry) => entry.key === item.key)) return;
    receivedAt.set(item.key, Date.now());
    items.value = mergeAnnouncerReel(items.peek(), [item]);
    refreshEntries();
  }

  const unbind = delegate(document.body, 'click', '[data-announcer-action]', (_event, target) => {
    const element = target as HTMLElement;
    const requestedAction = element.dataset.announcerAction;
    if (!requestedAction) return;
    switch (requestedAction) {
      case 'open':
        clearDiff();
        open.value = !open.peek();
        lastSpokenProject = undefined;
        if (open.peek()) void loadDigest();
        else {
          live.value = false;
          cancelSpeech();
          scheduleSettle();
        }
        break;
      case 'expand': {
        // The new size preset replaces any size the person dragged to, from where they put the surface:
        // reopening it under fresh wiring lets Kerf apply the preset and clamp it inside the viewport.
        const placement = loadAnnouncerFloatingPlacement(localStorage);
        if (placement) saveAnnouncerFloatingPlacement(localStorage, { x: placement.x, y: placement.y });
        const reopen = floating?.element;
        floating?.dispose();
        floating = undefined;
        if (reopen?.matches(':popover-open')) reopen.hidePopover();
        expanded.value = !expanded.peek();
        requestAnimationFrame(syncFloating);
        break;
      }
      case 'detach':
        setDetached(true);
        break;
      case 'reattach':
        expanded.value = false;
        setDetached(false);
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
        index.value = Math.min(entries.peek().length - 1, index.peek() + 1);
        break;
      case 'live':
        clearDiff();
        live.value = !live.peek();
        if (live.peek()) {
          markHeard();
          if (entries.peek().length) index.value = entries.peek().length - 1;
        }
        scheduleSettle();
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
          loadServerVoices();
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
        entries.value = [];
        receivedAt.clear();
        heard.clear();
        index.value = 0;
        scheduleSettle();
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
      const overlay = (presentation: 'docked' | 'corner' | 'floating') => (
        <AnnouncerOverlay
          open
          presentation={presentation}
          dockSize={dockSize.value}
          expanded={expanded.value}
          live={live.value}
          speaking={speaking.value}
          audible={audible.value}
          items={entries.value}
          index={index.value}
          error={error.value}
          currentDiff={currentDiff.value}
          diffLoading={diffLoading.value}
          diffError={diffError.value}
          providers={providers.value}
          provider={provider.value}
          voice={voice.value}
          voices={
            provider.value === 'browser'
              ? browserVoices.value
              : announcerVoiceChoices(serverVoices.value, navigator.language, voice.value)
          }
          rate={rate.value}
        />
      );
      return detached.value
        ? { floating: overlay('floating') }
        : { docked: overlay('docked'), corner: overlay('corner') };
    },
    onActivity,
    dispose: () => {
      if (settleTimer !== undefined) clearTimeout(settleTimer);
      settleTimer = undefined;
      floatingObserver.disconnect();
      floating?.dispose();
      floating = undefined;
      unrouteDock();
      unbind();
      unbindSettings();
      voicesSource?.removeEventListener('voiceschanged', refreshVoices);
      cancelSpeech();
      clearDiff();
    },
  };
}
