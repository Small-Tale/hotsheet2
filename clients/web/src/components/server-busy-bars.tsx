import './server-busy-bars.css';

export interface ServerBusyBarsProps {
  /** Number of bars to draw across the viewport (see `computeServerBusyBarCount`). */
  count: number;
  /** Whether the server is currently busy; drives visibility and animation. */
  busy: boolean;
}

/**
 * A decorative, full-width, 4px-tall overlay pinned to the top of the app that ripples a row
 * of yellow bars while the server is busy. It allocates no layout space (it is `position:fixed`)
 * and is inert to assistive technology and the pointer. The bar count is recomputed only on
 * resize by the host; each bar scales from 1px to 4px on a staggered cycle (HS2-MW1V3M).
 */
export function ServerBusyBars({ count, busy }: ServerBusyBarsProps) {
  const bars = Math.max(0, Math.trunc(count));
  return (
    <div class="server-busy-bars" data-component="server-busy-bars" data-visible={String(busy)} aria-hidden="true">
      {Array.from({ length: bars }, (_bar, index) => (
        <span class="server-busy-bars__bar" style={`--bar-index:${index}`}></span>
      ))}
    </div>
  );
}

export interface ServerBusyMessageProps {
  /** The human-readable description of the current server work. */
  message: string;
  /** Whether the label should be shown (busy AND the loading-activity setting is on). */
  visible: boolean;
}

/**
 * Optional loading-activity label: a small pill that hangs from the top edge (below the busy
 * bars) and names what the server is doing in human-readable terms (HS2-2G1Y8X). Off by default;
 * the host gates `visible` on the General settings toggle. Decorative/status-only.
 */
export function ServerBusyMessage({ message, visible }: ServerBusyMessageProps) {
  return (
    <div
      class="server-busy-message"
      data-component="server-busy-message"
      data-visible={String(visible && Boolean(message))}
      role="status"
      aria-live="polite"
    >
      {message}
    </div>
  );
}
