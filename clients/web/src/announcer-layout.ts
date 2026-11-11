import { clampRegionSize } from '@kerfjs/ui/resizable-region';

/** The docked announcer's ResizableRegion id; the shared region wiring routes its commits by it. */
export const ANNOUNCER_DOCK_REGION_ID = 'announcer-dock';

/** The docked announcer's height bounds (HS2-BS0XFK): tall enough for the header, one item, and the
 * controls; short enough that the sidebar summary and views stay reachable above it. */
export const ANNOUNCER_DOCK_BOUNDS = { min: 144, max: 480, fallback: 232 } as const;

const DOCK_SIZE_KEY = 'hotsheet.layout.announcer-dock.size';

export function normalizeAnnouncerDockSize(size: number): number {
  return Math.round(clampRegionSize(size, ANNOUNCER_DOCK_BOUNDS.min, ANNOUNCER_DOCK_BOUNDS.max));
}

/** The device's remembered docked height, or the default when none (or an unusable value) is stored. */
export function loadAnnouncerDockSize(storage: Pick<Storage, 'getItem'>): number {
  let stored: string | null;
  try {
    stored = storage.getItem(DOCK_SIZE_KEY);
  } catch {
    return ANNOUNCER_DOCK_BOUNDS.fallback;
  }
  const saved = Number(stored);
  return stored !== null && Number.isFinite(saved) && saved > 0
    ? normalizeAnnouncerDockSize(saved)
    : ANNOUNCER_DOCK_BOUNDS.fallback;
}

/** Store a committed docked height for this device; returns the bounded value the region shows. */
export function saveAnnouncerDockSize(storage: Pick<Storage, 'setItem'>, size: number): number {
  const normalized = normalizeAnnouncerDockSize(size);
  try {
    storage.setItem(DOCK_SIZE_KEY, String(normalized));
  } catch {
    // A full or blocked store still resizes the region for this session.
  }
  return normalized;
}
