import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { delegate, type Signal, signal } from 'kerfjs';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { Radio } from 'lucide';

import {
  announcerDigest,
  announcerItem,
  type AnnouncerReelItem,
  mergeAnnouncerReel,
  retainedAnnouncerReel,
} from '../announcer-reel';
import { type ActivityEvent, Api } from '../api';
import { AnnouncerOverlay } from '../components/announcer-overlay';
import type { Project } from '../interactions/types';

export interface AnnouncerController {
  trigger: () => SafeHtml;
  surface: () => ReturnType<typeof AnnouncerOverlay>;
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
  const provider = signal('browser');
  const voice = signal('');
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
          item.narration,
          provider.peek(),
          voice.peek() || undefined,
        );
        if (generation !== speechGeneration) return;
        currentUrl = URL.createObjectURL(blob);
        const audio = new Audio(currentUrl);
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
    const utterance = new SpeechSynthesisUtterance(item.narration);
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
    items.value = mergeAnnouncerReel(items.peek(), retainedAnnouncerReel(loaded));
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
        open.value = true;
        void loadDigest();
        break;
      case 'close':
        clearDiff();
        open.value = false;
        live.value = false;
        cancelSpeech();
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
      case 'select': {
        const selected = Number(element.dataset.index);
        if (Number.isInteger(selected) && selected >= 0 && selected < items.peek().length) {
          clearDiff();
          cancelSpeech();
          index.value = selected;
        }
        break;
      }
      case 'current-diff':
        void loadCurrentDiff();
        break;
      default:
        break;
    }
  });
  const unbindProvider = delegate(document.body, 'change', '[data-announcer-provider]', (_event, target) => {
    const selected = (target as HTMLSelectElement).value;
    if (selected === 'browser' || providers.peek().includes(selected)) {
      cancelSpeech();
      provider.value = selected;
    }
  });
  const unbindVoice = delegate(document.body, 'change', '[data-announcer-voice]', (_event, target) => {
    cancelSpeech();
    voice.value = (target as HTMLInputElement).value.trim();
  });
  return {
    trigger: () => (
      <ToolbarControlGroup single>
        <button
          type="button"
          aria-label="Announcer"
          title="Announcer"
          aria-pressed={open.value}
          data-announcer-action="open"
        >
          <LucideIcon icon={Radio} name="radio" />
        </button>
      </ToolbarControlGroup>
    ),
    surface: () => (
      <AnnouncerOverlay
        open={open.value}
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
      />
    ),
    onActivity,
    dispose: () => {
      unbind();
      unbindProvider();
      unbindVoice();
      cancelSpeech();
      clearDiff();
    },
  };
}
