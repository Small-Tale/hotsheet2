import type { CatalogBackgroundStyle } from '@kerfjs/ui/catalog';

/** The UX catalog's one persisted display record (HS2-HMVBZW). */
export const DEMO_DISPLAY_STORAGE_KEY = 'hotsheet.ux-demo.display';
const backgrounds: readonly string[] = ['checkerboard', 'vertical-stripes', 'layout-guide', 'surface', 'sunken'];

export interface DemoDisplayPreferences {
  theme?: 'light' | 'dark';
  increasedContrast?: boolean;
  reducedMotion?: boolean;
  backgroundStyle?: CatalogBackgroundStyle;
  collapsed?: boolean;
}

const optionalBoolean = (value: unknown) => (typeof value === 'boolean' ? value : undefined);

/** Read the saved catalog display choices, ignoring anything malformed or unreadable. */
export function readDemoDisplayPreferences(storage: Pick<Storage, 'getItem'> | null): DemoDisplayPreferences {
  try {
    const saved: unknown = JSON.parse(storage?.getItem(DEMO_DISPLAY_STORAGE_KEY) ?? 'null');
    if (saved === null || typeof saved !== 'object' || Array.isArray(saved)) return {};
    const values = saved as Record<string, unknown>;
    return {
      theme: values.theme === 'dark' || values.theme === 'light' ? values.theme : undefined,
      increasedContrast: optionalBoolean(values.increasedContrast),
      reducedMotion: optionalBoolean(values.reducedMotion),
      backgroundStyle:
        typeof values.backgroundStyle === 'string' && backgrounds.includes(values.backgroundStyle)
          ? (values.backgroundStyle as CatalogBackgroundStyle)
          : undefined,
      collapsed: optionalBoolean(values.collapsed),
    };
  } catch {
    return {};
  }
}

/** Save the catalog display choices; the live controls keep working when storage is unavailable. */
export function writeDemoDisplayPreferences(
  storage: Pick<Storage, 'setItem'> | null,
  preferences: DemoDisplayPreferences,
): void {
  try {
    storage?.setItem(DEMO_DISPLAY_STORAGE_KEY, JSON.stringify(preferences));
  } catch {
    // Blocked storage only loses persistence.
  }
}
