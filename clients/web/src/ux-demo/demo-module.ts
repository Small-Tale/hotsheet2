import type { SafeHtml } from 'kerfjs';

/** One catalog entry's demo: its stage content and, optionally, its settings form and Reset. */
export interface DemoEntry {
  /** Render the entry's examples in the catalog stage. */
  Demo: () => SafeHtml;
  /** Render the entry's adjustable settings in the settings inspector. */
  Settings?: () => SafeHtml;
  /** Restore the entry's settings and fixtures (the settings form's Reset action). */
  reset?: (root: HTMLElement) => void;
}

/**
 * A demo source file's contribution to the catalog: the entries it renders and, optionally, the
 * delegated interactions it installs on the catalog root (returning their disposer).
 */
export interface DemoModule {
  entries: Readonly<Record<string, DemoEntry>>;
  wire?: (root: HTMLElement) => () => void;
}

/** A Web Awesome or native form control read through its live `value`/`checked` properties. */
export type FormControl = HTMLElement & { checked: boolean; value: string };

/** Add a window, document, or element listener and return its remover, for a module's `wire` scope. */
export function listen<K extends keyof WindowEventMap>(
  target: Window,
  type: K,
  handler: (event: WindowEventMap[K]) => void,
  options?: AddEventListenerOptions,
): () => void;
export function listen<K extends keyof DocumentEventMap>(
  target: Document,
  type: K,
  handler: (event: DocumentEventMap[K]) => void,
  options?: AddEventListenerOptions,
): () => void;
export function listen<K extends keyof HTMLElementEventMap>(
  target: HTMLElement,
  type: K,
  handler: (event: HTMLElementEventMap[K]) => void,
  options?: AddEventListenerOptions,
): () => void;
export function listen(
  target: EventTarget,
  type: string,
  handler: (event: Event) => void,
  options?: AddEventListenerOptions,
): () => void {
  target.addEventListener(type, handler, options);
  return () => {
    target.removeEventListener(type, handler, options);
  };
}
