import { clampRegionSize } from './components/resizable-region';

export type AppRegionId = 'app-sidebar' | 'app-inspector' | 'app-terminal-drawer';

export const APP_REGION_BOUNDS: Record<AppRegionId, { min: number; max: number; fallback: number }> = {
  'app-sidebar': { min: 250, max: 360, fallback: 272 },
  'app-inspector': { min: 280, max: 520, fallback: 352 },
  // The drawer's real maximum is layout-dependent: the distance from the bottom
  // of the shell to the bottom of PageHeader. Keep persistence unbounded here;
  // MainShell supplies the measured maximum when rendering and resizing.
  'app-terminal-drawer': { min: 180, max: Number.POSITIVE_INFINITY, fallback: 320 },
};

const storageKey = (id: AppRegionId) => `hotsheet.layout.${id}.size`;

export function isAppRegionId(value: string | undefined): value is AppRegionId {
  return value === 'app-sidebar' || value === 'app-inspector' || value === 'app-terminal-drawer';
}

export function normalizeAppRegionSize(id: AppRegionId, size: number): number {
  const bounds = APP_REGION_BOUNDS[id];
  return clampRegionSize(size, bounds.min, bounds.max);
}

export function loadAppRegionSize(storage: Pick<Storage, 'getItem'>, id: AppRegionId): number {
  const saved = Number(storage.getItem(storageKey(id)));
  return Number.isFinite(saved) && saved > 0
    ? normalizeAppRegionSize(id, saved)
    : APP_REGION_BOUNDS[id].fallback;
}

export function saveAppRegionSize(storage: Pick<Storage, 'setItem'>, id: AppRegionId, size: number): number {
  const normalized = normalizeAppRegionSize(id, size);
  storage.setItem(storageKey(id), String(normalized));
  return normalized;
}

export function terminalDrawerMaximum(mainBottom: number, workAreaTop: number): number {
  return Math.max(APP_REGION_BOUNDS['app-terminal-drawer'].min, Math.floor(mainBottom - workAreaTop));
}
