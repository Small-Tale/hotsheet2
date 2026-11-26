import { List } from '@kerfjs/ui/list';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { StateBanner } from '@kerfjs/ui/state-banner';
import { ServerOff } from 'lucide';

import { homeOutageBanners } from '../orchestration-home-outage';
import type { OrchestrationTimeline } from '../orchestration-timeline';
import type { ClockFormat } from '../orchestration-timeline-board';

export interface OrchestrationHomeOutageProps {
  timeline: OrchestrationTimeline;
  clock: ClockFormat;
}

/**
 * HS2-EX594V: one warning banner per multi-site project whose home site is unavailable, above the
 * Orchestration timeline: which project, since when, why, what still runs and what to do. There is no
 * failover, so the banner offers no action; it leaves when the timeline no longer lists the outage.
 */
export function OrchestrationHomeOutage({ timeline, clock }: OrchestrationHomeOutageProps) {
  const banners = homeOutageBanners(timeline, clock);
  if (banners.length === 0) return <></>;
  return (
    <List controlInsets="trl" gap="xs" rootAttributes={{ 'data-orchestration-home-outages': String(banners.length) }}>
      {banners.map((banner) => (
        <StateBanner
          copyLayout="stacked"
          tone="warning"
          icon={<LucideIcon icon={ServerOff} name="server-off" />}
          title={banner.title}
          detail={banner.detail}
          rootAttributes={{ 'data-key': banner.key, 'data-home-outage': banner.key }}
        />
      ))}
    </List>
  );
}
