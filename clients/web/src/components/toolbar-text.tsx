import './toolbar-text.css';

export type ToolbarTextSize = 'large' | 'default' | 'small';

export interface ToolbarTextProps { text: string; size?: ToolbarTextSize; className?: string }

export function ToolbarText({ text, size = 'default', className = '' }: ToolbarTextProps) {
  return <span class={`toolbar-text ${className}`.trim()} data-component="toolbar-text" data-size={size}>{text}</span>;
}
