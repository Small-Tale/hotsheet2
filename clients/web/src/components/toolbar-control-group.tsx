import './toolbar-control-group.css';

import type { SafeHtml } from 'kerfjs/jsx-runtime';

export interface ToolbarControlGroupProps {
  children: SafeHtml | SafeHtml[];
  label?: string;
  className?: string;
  expanded?: boolean;
  single?: boolean;
  appearance?: 'contained' | 'borderless';
  tone?: 'default' | 'dark';
  buttonAppearance?: 'plain' | 'push';
}

export function ToolbarControlGroup({ children, label, className = '', expanded = false, single = false, appearance = 'contained', tone = 'default', buttonAppearance = 'plain' }: ToolbarControlGroupProps) {
  return <div class={`toolbar-control-group ${className}`.trim()} data-component="toolbar-control-group" role={label ? 'group' : undefined} aria-label={label} data-appearance={appearance} data-tone={tone} data-button-appearance={buttonAppearance} data-expanded={String(expanded)} data-single={String(single)}>{children}</div>;
}
