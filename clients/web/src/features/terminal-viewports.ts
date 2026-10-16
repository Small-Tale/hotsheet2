import type { Signal } from 'kerfjs';

import type { Project } from '../interactions/types';
import type { TerminalModifiers } from '../terminal-keys';
import { currentTerminalLimits } from '../terminal-limits';
import { ProgressiveTerminalWorkQueue } from '../terminal-progressive-work';
import {
  mountTerminalViewport,
  TERMINAL_VIEWPORT_PARK_EVENT,
  TERMINAL_VIEWPORT_PARKED_CLOSED_EVENT,
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
  const terminalViewportCandidates = new Set<HTMLElement>();
  const terminalViewportObservationTargets = new Map<HTMLElement, HTMLElement>();
  const terminalViewportCandidatesByTarget = new Map<HTMLElement, HTMLElement>();
  let terminalViewportObserver: IntersectionObserver | undefined;
  const terminalViewportWork = new ProgressiveTerminalWorkQueue<HTMLElement>({
    mountsPerTurn: 2,
    disposalsPerTurn: 2,
    schedule: (work) => requestAnimationFrame(() => window.setTimeout(work, 0)),
    mount: (element) => {
      mountTerminalViewportElement(element);
    },
    dispose: (work) => {
      work();
    },
  });

  function mountTerminalViewportElement(element: HTMLElement) {
    if (!element.isConnected || terminalViewportMounts.has(element)) return;
    stopObservingTerminalViewport(element);
    const projectId = element.dataset.projectId,
      terminalId = element.dataset.terminalId,
      current = projects.value.find((item) => item.id === projectId);
    if (!current || !terminalId) return;
    const interactive = element.dataset.displayMode === 'interactive',
      autoFocus =
        interactive &&
        (element.closest('.terminal-dashboard__magnified') !== null ||
          terminalViewportShouldAutoFocus(dependencies.pendingTerminalFocus, current.id, terminalId));
    terminalViewportMounts.set(
      element,
      mountTerminalViewport(element, {
        url: terminalBrowserWebSocketUrl(current.apiPath, terminalId),
        autoFocus,
        mobileColumns: dependencies.mobileTerminalColumns,
        modifiers: dependencies.terminalModifiers,
        onTicketReference: interactive
          ? (reference) => {
              const parsed = parseTicketLinkReference(reference);
              if (!parsed) return;
              dependencies.openTicketReference(element, parsed.slug, parsed.projectId, current.id);
            }
          : undefined,
      }),
    );
    if (autoFocus) dependencies.pendingTerminalFocus = undefined;
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
    enforceWarmTerminalLimit();
  }

  // Evict the oldest parked viewports beyond the warm-terminal limit; a lowered limit applies at once.
  function enforceWarmTerminalLimit() {
    const limit = Math.max(0, warmTerminalLimit());
    for (const [, oldest] of parkedViewports) {
      if (parkedViewports.size <= limit) break;
      oldest.evict();
    }
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
    for (const element of elements) {
      if (terminalViewportMounts.has(element) || terminalViewportCandidates.has(element)) continue;
      if (element.dataset.mountPolicy === 'keep-alive' && restoreParkedTerminalViewport(element)) continue;
      if (element.dataset.mountPolicy !== 'visible-progressive') {
        mountTerminalViewportElement(element);
        continue;
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
  }

  function parkProjectViewports(projectId: string) {
    for (const [element, dispose] of terminalViewportMounts) {
      if (element.dataset.projectId !== projectId || element.dataset.mountPolicy !== 'keep-alive') continue;
      const identity = terminalViewportIdentity(element);
      if (!identity) continue;
      terminalViewportMounts.delete(element);
      parkTerminalViewport(identity, element, dispose);
    }
  }

  return { syncTerminalViewportMounts, parkProjectViewports, enforceWarmTerminalLimit };
}
