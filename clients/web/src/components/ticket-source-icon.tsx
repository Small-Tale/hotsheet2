import './ticket-source-icon.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Plug } from 'lucide';

import { isTransparentCommandColor } from './customization-palette';
import { GIT_SOURCE_MARK, GITHUB_SOURCE_MARK } from './ticket-source-marks';

export interface TicketSourceIdentity {
  provider: string;
  name: string;
  color?: string;
}

/** Project-local source mark. The selected palette color fills the mark itself. */
export function TicketSourceIcon({
  source,
  size = 'list',
}: {
  source: TicketSourceIdentity;
  size?: 'list' | 'compact';
}) {
  const color = isTransparentCommandColor(source.color) ? undefined : source.color;
  return (
    <span
      class="ticket-source-icon"
      data-component="ticket-source-icon"
      data-size={size}
      data-provider={source.provider}
      title={`${source.name} source`}
      aria-label={`${source.name} source`}
      style={color ? `color: ${color}` : undefined}
    >
      {(() => {
        switch (source.provider) {
          case 'git':
          case 'github':
            return (
              <svg class="ticket-source-icon__mark" viewBox="0 0 100 100" fill="currentColor" aria-hidden="true">
                <path d={source.provider === 'git' ? GIT_SOURCE_MARK : GITHUB_SOURCE_MARK} />
              </svg>
            );
          case 'gitlab':
            return (
              <svg
                class="ticket-source-icon__mark"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
              >
                <path d="m12 22 10-8.1-2.3-11-3.5 6H7.8l-3.5-6L2 13.9 12 22Z" />
                <path d="m7.8 8.9 4.2 13.1 4.2-13.1" />
              </svg>
            );
          case 'jira':
            return (
              <svg
                class="ticket-source-icon__mark"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
                aria-hidden="true"
              >
                <path d="M12 2 22 12 12 22 2 12 12 2Z" />
                <path d="m12 7 5 5-5 5-5-5 5-5Z" />
              </svg>
            );
          default:
            return <LucideIcon icon={Plug} name={source.provider} size={size === 'list' ? 26.4 : 15.2} />;
        }
      })()}
    </span>
  );
}
