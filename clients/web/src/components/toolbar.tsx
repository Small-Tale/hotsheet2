import type { SafeHtml } from 'kerfjs/jsx-runtime';
import './toolbar.css';

export interface ToolbarProps {
  leading?: SafeHtml;
  center?: SafeHtml;
  trailing?: SafeHtml;
  label?: string;
  divider?: boolean;
  className?: string;
}

export function Toolbar({ leading, center, trailing, label, divider = true, className = '' }: ToolbarProps) {
  return <header class={`toolbar ${className}`.trim()} data-component="toolbar" data-divider={String(divider)} aria-label={label}>
    <div class="toolbar__leading">{leading}</div>
    <div class="toolbar__center">{center}</div>
    <div class="toolbar__trailing">{trailing}</div>
  </header>;
}
