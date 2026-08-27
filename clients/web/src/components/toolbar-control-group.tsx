import type { SafeHtml } from 'kerfjs/jsx-runtime';
import './toolbar-control-group.css';

export interface ToolbarControlGroupProps {
  children: SafeHtml | SafeHtml[];
  label?: string;
  className?: string;
  expanded?: boolean;
}

export function ToolbarControlGroup({ children, label, className = '', expanded = false }: ToolbarControlGroupProps) {
  return <div class={`toolbar-control-group ${className}`.trim()} role={label ? 'group' : undefined} aria-label={label} data-expanded={String(expanded)}>{children}</div>;
}
