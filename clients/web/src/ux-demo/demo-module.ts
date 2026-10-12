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
export type FormControl = HTMLElement & { checked: boolean; value: string };
