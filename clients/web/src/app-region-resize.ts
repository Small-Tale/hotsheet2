import { clampRegionSize } from '@kerfjs/ui/resizable-region';

/** The shell's Workbench panel ids (`<workbench id>-left-rail` etc., HS2-P289N2). */
export type AppRegionId = 'app-left-rail' | 'app-right-rail' | 'app-bottom-drawer';
export const TERMINAL_DRAWER_MIN_SIZE = 228;
export const TERMINAL_DRAWER_COLLAPSE_OVERSHOOT = 48;

export const APP_REGION_BOUNDS: Record<AppRegionId, { min: number; max: number; fallback: number }> = {
  'app-left-rail': { min: 250, max: 360, fallback: 272 },
  'app-right-rail': { min: 280, max: 520, fallback: 352 },
  // The drawer's real maximum is layout-dependent: the distance from the bottom
  // of the shell to the bottom of PageHeader. Keep persistence unbounded here;
  // MainShell supplies the measured maximum when rendering and resizing.
  'app-bottom-drawer': { min: TERMINAL_DRAWER_MIN_SIZE, max: Number.POSITIVE_INFINITY, fallback: 320 },
};

const storageKey = (id: AppRegionId) => `hotsheet.layout.${id}.size`;
/** Storage keys written before the shell moved onto Workbench panel ids; read once as a fallback. */
const LEGACY_STORAGE_KEYS: Record<AppRegionId, string> = {
  'app-left-rail': 'hotsheet.layout.app-sidebar.size',
  'app-right-rail': 'hotsheet.layout.app-inspector.size',
  'app-bottom-drawer': 'hotsheet.layout.app-terminal-drawer.size',
};

export function isAppRegionId(value: string | undefined): value is AppRegionId {
  return value === 'app-left-rail' || value === 'app-right-rail' || value === 'app-bottom-drawer';
}

export function normalizeAppRegionSize(id: AppRegionId, size: number): number {
  const bounds = APP_REGION_BOUNDS[id];
  return clampRegionSize(size, bounds.min, bounds.max);
}

export function loadAppRegionSize(storage: Pick<Storage, 'getItem'>, id: AppRegionId): number {
  const saved = Number(storage.getItem(storageKey(id)) ?? storage.getItem(LEGACY_STORAGE_KEYS[id]));
  return Number.isFinite(saved) && saved > 0 ? normalizeAppRegionSize(id, saved) : APP_REGION_BOUNDS[id].fallback;
}

export function saveAppRegionSize(storage: Pick<Storage, 'setItem'>, id: AppRegionId, size: number): number {
  const normalized = normalizeAppRegionSize(id, size);
  storage.setItem(storageKey(id), String(normalized));
  return normalized;
}

/**
 * The drawer's usable minimum. On a phone the expanded drawer owns the bottom screen edge and pads
 * its content by the home-indicator inset, so the minimum grows by that inset to keep the same
 * usable height (HS2-ZEC4QV).
 */
export function terminalDrawerMinimum(safeAreaBottom = 0): number {
  return TERMINAL_DRAWER_MIN_SIZE + Math.max(0, Math.round(Number.isFinite(safeAreaBottom) ? safeAreaBottom : 0));
}

export function terminalDrawerMaximum(
  mainBottom: number,
  workAreaTop: number,
  minimum = APP_REGION_BOUNDS['app-bottom-drawer'].min,
): number {
  return Math.max(minimum, Math.floor(mainBottom - workAreaTop));
}

export function terminalDrawerDragDecision(
  rawSize: number,
  maximum: number,
  minimum = TERMINAL_DRAWER_MIN_SIZE,
): { size: number; collapse: boolean } {
  return {
    size: Math.min(maximum, Math.max(minimum, Math.round(rawSize))),
    collapse: rawSize <= minimum - TERMINAL_DRAWER_COLLAPSE_OVERSHOOT,
  };
}
