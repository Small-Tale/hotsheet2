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

/** The detached announcer's PinnablePopoverSurface id (HS2-51X25Z). */
export const ANNOUNCER_FLOATING_ID = 'announcer-floating';

const DETACHED_KEY = 'hotsheet.layout.announcer-detached';
const FLOATING_PLACEMENT_KEY = 'hotsheet.layout.announcer-floating.placement';

/** Where the person last left the detached announcer: its top-left corner in viewport px and, once
 * resized, its size. Kerf's wirePinnablePopover reports it and takes it back to restore. */
export interface AnnouncerFloatingPlacement {
  readonly x: number;
  readonly y: number;
  readonly width?: number;
  readonly height?: number;
}

/** Whether this device last popped the announcer out of the sidebar into its floating dialog. */
export function loadAnnouncerDetached(storage: Pick<Storage, 'getItem'>): boolean {
  try {
    return storage.getItem(DETACHED_KEY) === 'true';
  } catch {
    return false;
  }
}

export function saveAnnouncerDetached(storage: Pick<Storage, 'setItem'>, detached: boolean): void {
  try {
    storage.setItem(DETACHED_KEY, String(detached));
  } catch {
    // A full or blocked store still detaches or reattaches for this session.
  }
}

const finite = (value: unknown): value is number => typeof value === 'number' && Number.isFinite(value);
const positive = (value: unknown): value is number => finite(value) && value > 0;

/** Keep only a usable placement: finite coordinates and, when present, a positive size. */
export function normalizeAnnouncerFloatingPlacement(value: unknown): AnnouncerFloatingPlacement | undefined {
  if (typeof value !== 'object' || value === null) return undefined;
  const { x, y, width, height } = value as Record<string, unknown>;
  if (!finite(x) || !finite(y)) return undefined;
  return {
    x: Math.round(x),
    y: Math.round(y),
    ...(positive(width) ? { width: Math.round(width) } : {}),
    ...(positive(height) ? { height: Math.round(height) } : {}),
  };
}

/** The device's remembered floating placement, or none (Kerf then centers the panel). */
export function loadAnnouncerFloatingPlacement(
  storage: Pick<Storage, 'getItem'>,
): AnnouncerFloatingPlacement | undefined {
  try {
    const stored = storage.getItem(FLOATING_PLACEMENT_KEY);
    return stored === null ? undefined : normalizeAnnouncerFloatingPlacement(JSON.parse(stored));
  } catch {
    return undefined;
  }
}

/** Store a reported placement for this device; returns what was kept (nothing for an unusable value). */
export function saveAnnouncerFloatingPlacement(
  storage: Pick<Storage, 'setItem'>,
  placement: unknown,
): AnnouncerFloatingPlacement | undefined {
  const normalized = normalizeAnnouncerFloatingPlacement(placement);
  if (!normalized) return undefined;
  try {
    storage.setItem(FLOATING_PLACEMENT_KEY, JSON.stringify(normalized));
  } catch {
    // A full or blocked store still keeps the placement for this session.
  }
  return normalized;
}
