import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { GripHorizontal, GripVertical } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './resizable-region.css';

export type ResizableRegionAxis = 'horizontal' | 'vertical';
export type ResizableRegionEdge = 'start' | 'end';
export interface ResizableRegionProps {
  id: string;
  label: string;
  size: number;
  min: number;
  max: number;
  axis?: ResizableRegionAxis;
  edge?: ResizableRegionEdge;
  collapsed?: boolean;
  children: SafeHtml | SafeHtml[];
}

export const clampRegionSize = (size: number, min: number, max: number) => Math.min(max, Math.max(min, Math.round(size)));

export function ResizableRegion({ id, label, size, min, max, axis = 'horizontal', edge = 'end', collapsed = false, children }: ResizableRegionProps) {
  const resolved = collapsed ? 0 : clampRegionSize(size, min, max);
  const orientation = axis === 'horizontal' ? 'vertical' : 'horizontal';
  return <section class="resizable-region" data-component="resizable-region" data-region-id={id} data-axis={axis} data-edge={edge} data-collapsed={String(collapsed)} style={`--resizable-region-size:${resolved}px`} aria-label={label}>
    <div class="resizable-region__content">{children}</div>
    <div class="resizable-region__handle" role="separator" tabindex="0" aria-label={`Resize ${label}`} aria-orientation={orientation} aria-valuemin={collapsed ? 0 : min} aria-valuemax={max} aria-valuenow={resolved} data-action="resize-region" data-region-id={id}>
      <LucideIcon icon={axis === 'horizontal' ? GripVertical : GripHorizontal} name={axis === 'horizontal' ? 'grip-vertical' : 'grip-horizontal'} />
    </div>
  </section>;
}
