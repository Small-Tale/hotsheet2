import type { SafeHtml } from 'kerfjs/jsx-runtime';
import './toolbar-control-group.css';

export interface ToolbarControlGroupProps {
  children: SafeHtml | SafeHtml[];
  label?: string;
  className?: string;
  expanded?: boolean;
  single?: boolean;
}

export function ToolbarControlGroup({ children, label, className = '', expanded = false, single = false }: ToolbarControlGroupProps) {
  return <div class={`toolbar-control-group ${className}`.trim()} role={label ? 'group' : undefined} aria-label={label} data-expanded={String(expanded)} data-single={String(single)}>{children}</div>;
}
