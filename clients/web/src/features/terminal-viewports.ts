import type { Signal } from 'kerfjs';

import type { Project } from '../interactions/types';
import { ProgressiveTerminalWorkQueue } from '../terminal-progressive-work';
import {
  mountTerminalViewport,
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
}

function terminalViewportIdentity(element: HTMLElement): string | undefined {
  const projectId = element.dataset.projectId,
    terminalId = element.dataset.terminalId;
  return projectId && terminalId ? `${projectId}\u0000${terminalId}` : undefined;
}

export function createTerminalViewportsController(dependencies: TerminalViewportsDependencies) {
  const { projects } = dependencies;
  const terminalViewportMounts = new Map<HTMLElement, () => void>();
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
    const elements = new Set(document.querySelectorAll<HTMLElement>('[data-component="terminal-viewport"]')),
      replacementIdentities = new Set(Array.from(elements, terminalViewportIdentity).filter(Boolean));
    for (const [element, dispose] of terminalViewportMounts)
      if (!elements.has(element)) {
        terminalViewportMounts.delete(element);
        if (replacementIdentities.has(terminalViewportIdentity(element))) dispose();
        else terminalViewportWork.enqueueDisposal(dispose);
      }
    for (const element of terminalViewportCandidates)
      if (!elements.has(element)) {
        terminalViewportWork.cancelMount(element);
        stopObservingTerminalViewport(element);
      }
    for (const element of elements) {
      if (terminalViewportMounts.has(element) || terminalViewportCandidates.has(element)) continue;
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

  return { syncTerminalViewportMounts };
}
