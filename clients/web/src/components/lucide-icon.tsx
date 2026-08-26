import type { IconNode } from 'lucide';
import { jsx } from 'kerfjs/jsx-runtime';

export interface LucideIconProps {
  icon: IconNode;
  name: string;
  class?: string;
}

/** Render official Lucide icon nodes through Kerf without copied SVG markup. */
export function LucideIcon({ icon, name, class: className }: LucideIconProps) {
  return (
    <svg
      class={className}
      data-lucide={name}
      aria-hidden="true"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
    >
      {icon.map(([tag, attrs]) => jsx(tag, attrs))}
    </svg>
  );
}
