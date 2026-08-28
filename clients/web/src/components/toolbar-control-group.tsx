import type { SafeHtml } from 'kerfjs/jsx-runtime';
import './toolbar-control-group.css';

export interface ToolbarControlGroupProps {
  children: SafeHtml | SafeHtml[];
  label?: string;
  className?: string;
  expanded?: boolean;
  single?: boolean;
  appearance?: 'contained' | 'borderless';
}

export function ToolbarControlGroup({ children, label, className = '', expanded = false, single = false, appearance = 'contained' }: ToolbarControlGroupProps) {
  return <div class={`toolbar-control-group ${className}`.trim()} data-component="toolbar-control-group" role={label ? 'group' : undefined} aria-label={label} data-appearance={appearance} data-expanded={String(expanded)} data-single={String(single)}>{children}</div>;
}
