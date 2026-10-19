import type { Signal } from 'kerfjs';

import { createDedicatedTerminalTileViewport } from '../components/terminal-dashboard';
import type { Project } from '../interactions/types';
import type { TerminalModifiers } from '../terminal-keys';
import { currentTerminalLimits } from '../terminal-limits';
import { ProgressiveTerminalWorkQueue } from '../terminal-progressive-work';
import {
  mountTerminalViewport,
  TERMINAL_VIEWPORT_PARK_EVENT,
  TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT,
  TERMINAL_VIEWPORT_PRESENTATION_EVENT,
  TERMINAL_VIEWPORT_REPARENT_EVENT,
  TERMINAL_VIEWPORT_RESUME_EVENT,
  terminalBrowserWebSocketUrl,
  type TerminalFocusRequest,
  terminalViewportShouldAutoFocus,
} from '../terminal-viewport';
import { parseTicketLinkReference } from '../ticket-link-resolution';

export interface TerminalViewportsDependencies {
  projects: Signal<Project[]>;
  pendingTerminalFocus: TerminalFocusRequest | undefined;
  openTicketReference: (
    element: HTMLElement,
    slug: string,
    projectId: string | undefined,
    preferredProject: string,
  ) => void;
  /** Current phone-width terminal column preference (HS2-WMN626). */
  mobileTerminalColumns?: () => number;
  /** Sticky key-bar modifiers shared by every interactive terminal (HS2-CKS78M). */
  terminalModifiers?: { current: () => TerminalModifiers; consume: () => void };
  /** Where kept-alive viewports wait while out of the page; defaults to a hidden body container. */
  parking?: TerminalViewportParking;
  /** How many kept-alive viewports stay warm out of the page; defaults to the configured, memory-aware
   * warm-terminal limit (HS2-Y5DYC0). */
  warmTerminalLimit?: () => number;
  /** Build a dedicated viewport for a never-opened tile; injectable for ownership tests. */
  createTileViewport?: (host: HTMLElement) => HTMLElement;
}

/** DOM moves for kept-alive viewports, injectable so the ownership logic is testable. */
export interface TerminalViewportParking {
  /** Move a live viewport out of the page into a hidden holding area. */
  park: (element: HTMLElement) => void;
  /** Put a parked viewport back in the page in place of its freshly rendered placeholder. */
  restore: (placeholder: HTMLElement, parked: HTMLElement) => void;
}

/**
 * What a live viewport fixed at mount: its display mode, policies, presentation class, and whether it
 * sits in a magnified card. A re-rendered placeholder with the same identity and configuration can
 * adopt the live viewport instead of mounting a new emulator and socket (HS2-AMNNBR).
 */
export function terminalViewportConfiguration(element: HTMLElement): string {
  return [
    element.dataset.displayMode ?? '',
    element.dataset.mountPolicy ?? '',
    element.dataset.gridPolicy ?? '',
    element.dataset.mobileGridPolicy ?? '',
    element.classList.contains('terminal-viewport--dedicated') ? 'dedicated' : '',
    element.classList.contains('terminal-viewport--scaled-preview') ? 'scaled' : '',
    element.closest('[data-fixed-aspect-terminal-card="magnified"]') ? 'magnified' : '',
  ].join('|');
}

const documentParking: TerminalViewportParking = {
  park(element) {
    let holder = document.querySelector<HTMLElement>('[data-terminal-viewport-parking]');
    if (!holder) {
      holder = document.createElement('div');
      holder.hidden = true;
      holder.setAttribute('aria-hidden', 'true');
      holder.dataset.terminalViewportParking = '';
      document.body.append(holder);
    }
    holder.append(element);
  },
  restore(placeholder, parked) {
    placeholder.replaceWith(parked);
  },
};

/**
 * A grid tile's preview placeholder (the Workspace Grid and the drawer's project grid), which can host a
 * terminal's live dedicated instance instead of mounting a separate preview emulator (HS2-EYD24K). A
 * magnified card and the standalone TerminalPreview keep their own viewports.
 */
export function terminalTilePreview(element: HTMLElement): boolean {
  return (
    element.dataset.displayMode === 'scaled-preview' &&
    element.dataset.mountPolicy === 'visible-progressive' &&
    element.closest('[data-component="terminal-tile"]') !== null &&
    element.closest('[data-fixed-aspect-terminal-card="magnified"]') === null
  );
}

function terminalViewportIdentity(element: HTMLElement): string | undefined {
  const projectId = element.dataset.projectId,
    terminalId = element.dataset.terminalId;
  return projectId && terminalId ? `${projectId}\u0000${terminalId}` : undefined;
}

export function createTerminalViewportsController(dependencies: TerminalViewportsDependencies) {
  const { projects } = dependencies,
    parking = dependencies.parking ?? documentParking,
    warmTerminalLimit = dependencies.warmTerminalLimit ?? (() => currentTerminalLimits().warmTerminals);
  const terminalViewportMounts = new Map<HTMLElement, () => void>();
  // Kept-alive viewports out of the page, keyed by identity, oldest first (HS2-WGTQ6X).
  const parkedViewports = new Map<string, { element: HTMLElement; dispose: () => void; evict: () => void }>();
  // Grid tiles showing a terminal's live dedicated instance, keyed by the tile placeholder (HS2-EYD24K).
  // The instance sits inside the morph-skipped placeholder, so a re-render never clobbers it.
  const hostedViewports = new Map<HTMLElement, { identity: string; element: HTMLElement; dispose: () => void }>();
  // Instances currently in tile presentation, including ones parked straight out of a tile.
  const tiledInstances = new WeakSet<HTMLElement>();
  function presentInDrawer(element: HTMLElement) {
    if (!tiledInstances.delete(element)) return;
    element.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_PRESENTATION_EVENT, { detail: { tile: false } }));
  }
  const terminalViewportCandidates = new Set<HTMLElement>();
  const terminalViewportObservationTargets = new Map<HTMLElement, HTMLElement>();
  const terminalViewportCandidatesByTarget = new Map<HTMLElement, HTMLElement>();
  let terminalViewportObserver: IntersectionObserver | undefined;
  const terminalViewportWork = new ProgressiveTerminalWorkQueue<HTMLElement>({
    mountsPerTurn: 2,
    disposalsPerTurn: 2,
    schedule: (work) => requestAnimationFrame(() => window.setTimeout(work, 0)),
    mount: (element) => {
      if (terminalTilePreview(element) && mountFreshTileInstance(element)) return;
      mountTerminalViewportElement(element);
    },
    dispose: (work) => {
      work();
    },
  });

  function mountTerminalViewportElement(
    element: HTMLElement,
    initialTilePresentation = false,
  ): (() => void) | undefined {
    if (!element.isConnected || terminalViewportMounts.has(element)) return;
    stopObservingTerminalViewport(element);
    const projectId = element.dataset.projectId,
      terminalId = element.dataset.terminalId,
      current = projects.value.find((item) => item.id === projectId);
    if (!current || !terminalId) return;
    const interactive = element.dataset.displayMode === 'interactive',
      autoFocus =
        interactive &&
        !initialTilePresentation &&
        (element.closest('.terminal-dashboard__magnified') !== null ||
          terminalViewportShouldAutoFocus(dependencies.pendingTerminalFocus, current.id, terminalId));
    const dispose = mountTerminalViewport(element, {
      url: terminalBrowserWebSocketUrl(current.apiPath, terminalId),
      autoFocus,
      initialTilePresentation,
      mobileColumns: dependencies.mobileTerminalColumns,
      modifiers: dependencies.terminalModifiers,
      onTicketReference: interactive
        ? (reference) => {
            const parsed = parseTicketLinkReference(reference);
            if (!parsed) return;
            dependencies.openTicketReference(element, parsed.slug, parsed.projectId, current.id);
          }
        : undefined,
    });
    terminalViewportMounts.set(element, dispose);
    if (autoFocus) dependencies.pendingTerminalFocus = undefined;
    return dispose;
  }

  // A project switch (or hiding the drawer) takes a dedicated terminal out of the page. Keep its
  // emulator and socket warm instead of disposing them, so returning shows content at once.
  function parkTerminalViewport(identity: string, element: HTMLElement, dispose: () => void) {
    parkedViewports.get(identity)?.evict();
    const evict = () => {
      element.removeEventListener(TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT, evict);
      if (parkedViewports.get(identity)?.element !== element) return;
      parkedViewports.delete(identity);
      element.remove();
      dispose();
    };
    parking.park(element);
    element.dataset.parked = 'true';
    element.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_PARK_EVENT));
    element.addEventListener(TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT, evict);
    parkedViewports.set(identity, { element, dispose, evict });
    trimWarmTerminalLimit();
  }

  // Evict the oldest parked viewports beyond the warm-terminal limit; a lowered limit applies at once.
  // Instances shown in grid tiles count too: parked and tile-hosted instances share the limit, the oldest
  // parked going first, then the oldest tile falling back to its lightweight preview (HS2-EYD24K).
  function trimWarmTerminalLimit() {
    const limit = Math.max(0, warmTerminalLimit());
    for (const [, oldest] of parkedViewports) {
      if (parkedViewports.size + hostedViewports.size <= limit) break;
      oldest.evict();
    }
    for (const [host, hosted] of hostedViewports) {
      if (hostedViewports.size <= limit) break;
      releaseTileHost(host);
      terminalViewportMounts.delete(hosted.element);
      hosted.element.remove();
      hosted.dispose();
      considerTerminalViewportMount(host);
    }
  }

  function hasDedicatedInstance(identity: string): boolean {
    if (parkedViewports.has(identity) || [...hostedViewports.values()].some((entry) => entry.identity === identity))
      return true;
    return [...terminalViewportMounts.keys()].some(
      (element) => element.dataset.mountPolicy === 'keep-alive' && terminalViewportIdentity(element) === identity,
    );
  }

  function hasWarmRoom(): boolean {
    return parkedViewports.size + hostedViewports.size < Math.max(0, warmTerminalLimit());
  }

  function prepareTileHost(host: HTMLElement) {
    host.removeAttribute('style');
    host.style.width = '100%';
    host.style.height = '100%';
    host.style.display = 'block';
    host.dataset.hostingInstance = 'true';
    host.dataset.geometryReady = 'true';
  }

  // A never-opened terminal starts with the same dedicated viewport the drawer will later adopt.
  // Only the visible-progressive mount reaches here; overflow tiles retain the lightweight preview.
  function mountFreshTileInstance(host: HTMLElement): boolean {
    const identity = terminalViewportIdentity(host);
    if (!identity || !host.isConnected || !hasWarmRoom() || hasDedicatedInstance(identity)) return false;
    stopObservingTerminalViewport(host);
    const element = dependencies.createTileViewport
      ? dependencies.createTileViewport(host)
      : createDedicatedTerminalTileViewport(host.dataset.projectId ?? '', host.dataset.terminalId ?? '');
    prepareTileHost(host);
    host.append(element);
    const dispose = mountTerminalViewportElement(element, true);
    if (!dispose) {
      element.remove();
      releaseTileHost(host);
      return false;
    }
    hostedViewports.set(host, { identity, element, dispose });
    tiledInstances.add(element);
    return true;
  }

  // Show a parked instance inside a grid tile: the same emulator and socket, scaled and non-driving.
  function hostInTile(host: HTMLElement, identity: string): boolean {
    const entry = parkedViewports.get(identity);
    if (!entry) return false;
    parkedViewports.delete(identity);
    entry.element.removeEventListener(TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT, entry.evict);
    delete entry.element.dataset.parked;
    // The tile's own fixed grid is never mounted, so its paint guard must not hide the hosted emulator.
    prepareTileHost(host);
    host.append(entry.element);
    hostedViewports.set(host, { identity, element: entry.element, dispose: entry.dispose });
    terminalViewportMounts.set(entry.element, entry.dispose);
    tiledInstances.add(entry.element);
    entry.element.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_PRESENTATION_EVENT, { detail: { tile: true } }));
    return true;
  }

  function releaseTileHost(host: HTMLElement) {
    hostedViewports.delete(host);
    delete host.dataset.hostingInstance;
    host.dataset.geometryReady = 'false';
    host.setAttribute('aria-hidden', 'true');
    host.removeAttribute('style');
  }

  function promoteTilePreviews() {
    for (const [host, dispose] of terminalViewportMounts) {
      if (!hasWarmRoom()) break;
      if (!terminalTilePreview(host) || !host.isConnected) continue;
      const identity = terminalViewportIdentity(host);
      if (!identity || hasDedicatedInstance(identity)) continue;
      terminalViewportMounts.delete(host);
      dispose();
      if (!mountFreshTileInstance(host)) considerTerminalViewportMount(host);
    }
  }

  function enforceWarmTerminalLimit() {
    trimWarmTerminalLimit();
    promoteTilePreviews();
  }

  // The drawer takes the instance back from a tile; the tile falls back to its lightweight preview.
  function reclaimFromTile(placeholder: HTMLElement): HTMLElement | undefined {
    const identity = terminalViewportIdentity(placeholder),
      found =
        identity === undefined ? undefined : [...hostedViewports].find(([, hosted]) => hosted.identity === identity);
    if (!found) return undefined;
    const [host, hosted] = found;
    releaseTileHost(host);
    placeholder.replaceWith(hosted.element);
    terminalViewportMounts.set(hosted.element, hosted.dispose);
    presentInDrawer(hosted.element);
    const focus = terminalViewportShouldAutoFocus(
      dependencies.pendingTerminalFocus,
      placeholder.dataset.projectId ?? '',
      placeholder.dataset.terminalId ?? '',
    );
    hosted.element.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_RESUME_EVENT, { detail: { focus } }));
    if (focus) dependencies.pendingTerminalFocus = undefined;
    return host;
  }

  function restoreParkedTerminalViewport(placeholder: HTMLElement): boolean {
    const identity = terminalViewportIdentity(placeholder),
      entry = identity === undefined ? undefined : parkedViewports.get(identity);
    if (!identity || !entry) return false;
    parkedViewports.delete(identity);
    entry.element.removeEventListener(TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT, entry.evict);
    parking.restore(placeholder, entry.element);
    delete entry.element.dataset.parked;
    terminalViewportMounts.set(entry.element, entry.dispose);
    // An instance parked out of a grid tile returns to drawer presentation (HS2-EYD24K).
    presentInDrawer(entry.element);
    const focus = terminalViewportShouldAutoFocus(
      dependencies.pendingTerminalFocus,
      placeholder.dataset.projectId ?? '',
      placeholder.dataset.terminalId ?? '',
    );
    entry.element.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_RESUME_EVENT, { detail: { focus } }));
    if (focus) dependencies.pendingTerminalFocus = undefined;
    return true;
  }

  function stopObservingTerminalViewport(element: HTMLElement) {
    const target = terminalViewportObservationTargets.get(element);
    if (target) {
      terminalViewportObserver?.unobserve(target);
      terminalViewportCandidatesByTarget.delete(target);
    }
    terminalViewportObservationTargets.delete(element);
    terminalViewportCandidates.delete(element);
  }

  function ensureTerminalViewportObserver() {
    if (terminalViewportObserver || typeof IntersectionObserver === 'undefined') return terminalViewportObserver;
    terminalViewportObserver = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          const element = terminalViewportCandidatesByTarget.get(entry.target as HTMLElement);
          if (!element) continue;
          if (entry.isIntersecting) terminalViewportWork.enqueueMount(element);
          else terminalViewportWork.cancelMount(element);
        }
      },
      { rootMargin: '160px' },
    );
    return terminalViewportObserver;
  }

  function syncTerminalViewportMounts() {
    const elements = new Set(
        Array.from(document.querySelectorAll<HTMLElement>('[data-component="terminal-viewport"]')).filter(
          (element) => element.dataset.parked !== 'true',
        ),
      ),
      replacementIdentities = new Set(Array.from(elements, terminalViewportIdentity).filter(Boolean)),
      openProjects = new Set(projects.value.map((item) => item.id));
    // A tile that left the page (or lost its instance) no longer hosts it; the instance parks below.
    for (const [host, hosted] of hostedViewports)
      if (!host.isConnected || hosted.element.parentElement !== host) releaseTileHost(host);
    // A re-render that moved a live viewport's container (a grid-mode switch, full screen) leaves a
    // fresh placeholder with the same identity: move the live viewport into it rather than disposing
    // and recreating its emulator and socket (HS2-AMNNBR, HS2-TBQQ00). Keep-alive viewports already
    // survive this through parking.
    const placeholders = new Map<string, HTMLElement>();
    for (const element of elements) {
      const identity = terminalViewportIdentity(element);
      if (
        !identity ||
        element.dataset.mountPolicy === 'keep-alive' ||
        hostedViewports.has(element) ||
        terminalViewportMounts.has(element) ||
        terminalViewportCandidates.has(element)
      )
        continue;
      placeholders.set(`${identity}\u0000${terminalViewportConfiguration(element)}`, element);
    }
    for (const [element, dispose] of terminalViewportMounts)
      if (!elements.has(element)) {
        terminalViewportMounts.delete(element);
        const identity = terminalViewportIdentity(element);
        const adoptKey =
            identity && element.dataset.mountPolicy !== 'keep-alive'
              ? `${identity}\u0000${terminalViewportConfiguration(element)}`
              : undefined,
          placeholder = adoptKey ? placeholders.get(adoptKey) : undefined;
        if (adoptKey && placeholder) {
          placeholders.delete(adoptKey);
          elements.delete(placeholder);
          placeholder.replaceWith(element);
          elements.add(element);
          terminalViewportMounts.set(element, dispose);
          element.dispatchEvent(new CustomEvent(TERMINAL_VIEWPORT_REPARENT_EVENT));
          continue;
        }
        if (
          identity &&
          element.dataset.mountPolicy === 'keep-alive' &&
          openProjects.has(element.dataset.projectId ?? '')
        )
          parkTerminalViewport(identity, element, dispose);
        else if (replacementIdentities.has(identity)) dispose();
        else terminalViewportWork.enqueueDisposal(dispose);
      }
    for (const [, entry] of parkedViewports)
      if (!openProjects.has(entry.element.dataset.projectId ?? '')) entry.evict();
    for (const element of terminalViewportCandidates)
      if (!elements.has(element)) {
        terminalViewportWork.cancelMount(element);
        stopObservingTerminalViewport(element);
      }
    // One instance per terminal (HS2-EYD24K): a grid tile whose terminal has a warm instance shows that
    // instance instead of a separate preview, unless the drawer asks for it in this same pass.
    const drawerWanted = new Set(
      Array.from(elements)
        .filter((element) => element.dataset.mountPolicy === 'keep-alive' && !terminalViewportMounts.has(element))
        .map(terminalViewportIdentity),
    );
    for (const element of elements) {
      const identity = terminalViewportIdentity(element);
      if (
        !identity ||
        hostedViewports.has(element) ||
        drawerWanted.has(identity) ||
        !parkedViewports.has(identity) ||
        !terminalTilePreview(element)
      )
        continue;
      if (terminalViewportCandidates.has(element)) {
        terminalViewportWork.cancelMount(element);
        stopObservingTerminalViewport(element);
      }
      const preview = terminalViewportMounts.get(element);
      if (preview) {
        terminalViewportMounts.delete(element);
        preview();
      }
      hostInTile(element, identity);
    }
    const releasedHosts: HTMLElement[] = [];
    for (const element of elements) {
      if (
        hostedViewports.has(element) ||
        terminalViewportMounts.has(element) ||
        terminalViewportCandidates.has(element)
      )
        continue;
      if (element.dataset.mountPolicy === 'keep-alive') {
        const host = reclaimFromTile(element);
        if (host) {
          releasedHosts.push(host);
          continue;
        }
        if (restoreParkedTerminalViewport(element)) continue;
      }
      considerTerminalViewportMount(element);
    }
    // A tile still on screen after the drawer took its instance back shows the lightweight preview.
    for (const host of releasedHosts) considerTerminalViewportMount(host);
    promoteTilePreviews();
  }

  function considerTerminalViewportMount(element: HTMLElement) {
    if (
      !element.isConnected ||
      hostedViewports.has(element) ||
      terminalViewportMounts.has(element) ||
      terminalViewportCandidates.has(element)
    )
      return;
    if (element.dataset.mountPolicy !== 'visible-progressive') {
      mountTerminalViewportElement(element);
      return;
    }
    terminalViewportCandidates.add(element);
    const observer = ensureTerminalViewportObserver(),
      target = element.closest<HTMLElement>('[data-component="terminal-tile"]') ?? element;
    if (observer) {
      terminalViewportObservationTargets.set(element, target);
      terminalViewportCandidatesByTarget.set(target, element);
      observer.observe(target);
    } else terminalViewportWork.enqueueMount(element);
  }

  function parkProjectViewports(projectId: string) {
    for (const [element, dispose] of terminalViewportMounts) {
      if (element.dataset.projectId !== projectId || element.dataset.mountPolicy !== 'keep-alive') continue;
      const identity = terminalViewportIdentity(element);
      if (!identity) continue;
      terminalViewportMounts.delete(element);
      for (const [host, hosted] of hostedViewports) if (hosted.element === element) releaseTileHost(host);
      parkTerminalViewport(identity, element, dispose);
    }
  }

  return { syncTerminalViewportMounts, parkProjectViewports, enforceWarmTerminalLimit };
}
