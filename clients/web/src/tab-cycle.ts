/**
 * Pure next/previous cycling over a fixed, ordered list of tab ids, shared by the project-tab and
 * bottom-drawer-tab keyboard shortcuts (HS2-9SHYWD). Kept isolated from the DOM so the wrap-around
 * behaviour is unit-tested directly; the caller resolves `order`/`current` from live signals.
 */

/**
 * The id to move to when cycling from `current` by `direction` (`1` = next, `-1` = previous), wrapping
 * around the ends. Returns `undefined` for an empty `order`. When `current` is absent from `order`
 * (nothing selected yet, or a stale selection), the cycle enters at the first id going forward and the
 * last id going backward, so a shortcut always has a defined starting point.
 */
export function cycleTabId(order: readonly string[], current: string | undefined, direction: 1 | -1): string | undefined {
  if (order.length === 0) return undefined;
  const index = current === undefined ? -1 : order.indexOf(current);
  if (index < 0) return direction === 1 ? order[0] : order[order.length - 1];
  return order[(index + direction + order.length) % order.length];
}
