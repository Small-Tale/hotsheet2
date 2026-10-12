import './ticket-source-icon.css';

import { LucideIcon, type LucideNode } from '@kerfjs/ui/lucide-icon';
import { Plug } from 'lucide';

import { resolveTicketSourceColor } from './customization-palette';
import { GIT_SOURCE_MARK, GITHUB_SOURCE_MARK } from './ticket-source-marks';

export interface TicketSourceIdentity {
  provider: string;
  name: string;
  color?: string;
}

/** The supplied filled marks are drawn on a 100-unit grid; scale them into Lucide's 24-unit box. */
const filledMark = (d: string): LucideNode => [
  ['path', { d, fill: 'currentColor', stroke: 'none', transform: 'scale(0.24)' }],
];

/** Lucide-compatible source marks, so Kerf LucideIcon owns their geometry and color. */
const SOURCE_MARKS: Readonly<Record<string, LucideNode>> = {
  git: filledMark(GIT_SOURCE_MARK),
  github: filledMark(GITHUB_SOURCE_MARK),
  gitlab: [
    ['path', { d: 'm12 22 10-8.1-2.3-11-3.5 6H7.8l-3.5-6L2 13.9 12 22Z', 'stroke-width': '1.8' }],
    ['path', { d: 'm7.8 8.9 4.2 13.1 4.2-13.1', 'stroke-width': '1.8' }],
  ],
  jira: [
    ['path', { d: 'M12 2 22 12 12 22 2 12 12 2Z', 'stroke-width': '1.8', 'stroke-linejoin': 'miter' }],
    ['path', { d: 'm12 7 5 5-5 5-5-5 5-5Z', 'stroke-width': '1.8', 'stroke-linejoin': 'miter' }],
  ],
};

/** Project-local source mark. The selected palette color fills the mark itself. */
export function TicketSourceIcon({
  source,
  size = 'list',
}: {
  source: TicketSourceIdentity;
  size?: 'list' | 'compact';
}) {
  const color = resolveTicketSourceColor(source.color);
  return (
    <span
      class="ticket-source-icon"
      data-component="ticket-source-icon"
      data-size={size}
      data-provider={source.provider}
      title={`${source.name} source`}
      aria-label={`${source.name} source`}
      style={`color: ${color}`}
    >
      <LucideIcon
        icon={Object.hasOwn(SOURCE_MARKS, source.provider) ? SOURCE_MARKS[source.provider] : Plug}
        name={source.provider}
        size={size === 'list' ? 26.4 : 15.2}
      />
    </span>
  );
}
