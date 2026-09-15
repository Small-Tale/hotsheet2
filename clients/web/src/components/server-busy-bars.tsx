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
  return <div class="server-busy-bars" data-component="server-busy-bars" data-visible={String(busy)} aria-hidden="true">
    {Array.from({ length: bars }, (_bar, index) => <span class="server-busy-bars__bar" style={`--bar-index:${index}`}></span>)}
  </div>;
}
