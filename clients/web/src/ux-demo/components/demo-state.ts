import type { CatalogBackgroundStyle } from '@kerfjs/ui/catalog';
import { signal } from 'kerfjs';

import { findDemo } from '../catalog';
import { readDemoDisplayPreferences, writeDemoDisplayPreferences } from '../display-preferences';

/** The catalog's shared controlled state: the selected entry and the Catalog display choices. */
export const DEFAULT_DEMO = 'tag-chip';
export const URL_PARAM = 'component';

// Unit tests import the demo modules without a page, so the URL is optional here.
const requestedDemo = findDemo(
  typeof location === 'undefined' ? '' : (new URL(location.href).searchParams.get(URL_PARAM) ?? ''),
);
export const selectedId = signal(requestedDemo?.id ?? DEFAULT_DEMO);
/** Kerf's Catalog settings pane starts collapsed and collapses again on every selection (KF-0AY6AE). */
export const settingsCollapsed = signal(true);
/** The local project directory Kerf's UX Review capture link files under; unset on static hosting (HS2-41YZSB). */
export const uxReviewProject = signal<string | null>(null);

let displayStorage: Storage | null = null;
try {
  displayStorage = globalThis.localStorage;
} catch {
  // Blocked storage only loses persistence.
}
const savedDisplay = readDemoDisplayPreferences(displayStorage);
export const collapsed = signal(savedDisplay.collapsed ?? false);
export const theme = signal<'light' | 'dark'>(savedDisplay.theme ?? 'light');
export const increasedContrast = signal(savedDisplay.increasedContrast ?? false);
export const reducedMotion = signal(savedDisplay.reducedMotion ?? false);
export const backgroundStyle = signal<CatalogBackgroundStyle>(savedDisplay.backgroundStyle ?? 'checkerboard');

export function saveDisplayPreferences(): void {
  writeDemoDisplayPreferences(displayStorage, {
    theme: theme.value,
    increasedContrast: increasedContrast.value,
    reducedMotion: reducedMotion.value,
    backgroundStyle: backgroundStyle.value,
    collapsed: collapsed.value,
  });
}

/** Project the page-level display choices onto the document: Web Awesome's dark class, the Hot Sheet theme
 * attribute, and the demo-owned contrast and reduced-motion token overrides. */
export function applyDisplayPreferences(root: HTMLElement = document.documentElement): void {
  root.classList.toggle('wa-dark', theme.value === 'dark');
  root.dataset.theme = theme.value;
  root.classList.toggle('demo-contrast', increasedContrast.value);
  root.classList.toggle('demo-reduced-motion', reducedMotion.value);
}

/** The ticket-row context menu and the project-tab context menu the shell renders above every demo. */
export const contextMenu = signal<{ x: number; y: number; ticketSlug?: string } | undefined>(undefined);
export const tabContextMenu = signal<{ x: number; y: number; projectId: string } | undefined>(undefined);

const selectionListeners = new Set<(id: string) => void>();

/** Run `listener` after every selection change, whether from the catalog or a demo's own navigation. */
export function onDemoSelected(listener: (id: string) => void): () => void {
  selectionListeners.add(listener);
  return () => {
    selectionListeners.delete(listener);
  };
}

/** Select a catalog entry and mirror it into `?component=` with `replaceState`, as `wireCatalog` does. */
export function selectDemo(id: string): void {
  if (!findDemo(id)) return;
  selectedId.value = id;
  settingsCollapsed.value = true;
  contextMenu.value = undefined;
  const url = new URL(location.href);
  url.searchParams.set(URL_PARAM, id);
  history.replaceState(null, '', url);
  for (const listener of selectionListeners) listener(id);
}
