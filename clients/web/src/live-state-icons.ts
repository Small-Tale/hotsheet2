import { Activity, CirclePause, Hand, Hourglass, type IconNode, Moon, OctagonAlert, ZapOff } from 'lucide';

import type { LiveWord } from './api.js';

/** One official Lucide asset: the node for `LucideIcon`'s `icon` and its kebab-case `name`. */
export interface GlanceIcon {
  readonly icon: IconNode;
  readonly name: string;
}

/**
 * The standardized glance-surface state glyphs (HS2-CSPKQX, docs/34-glance-surfaces.md §34.10).
 * Every glance surface (standby, project sidebar, grid sidebar, Stats, away digest) takes a live
 * state's glyph from here, so one state never shows two icons.
 */
export const LIVE_STATE_ICONS: Readonly<Record<LiveWord, GlanceIcon>> = {
  active: { icon: Activity, name: 'activity' },
  waiting_on_you: { icon: Hand, name: 'hand' },
  may_be_stuck: { icon: Hourglass, name: 'hourglass' },
  stalled: { icon: OctagonAlert, name: 'octagon-alert' },
  halted: { icon: ZapOff, name: 'zap-off' },
  stopped: { icon: CirclePause, name: 'circle-pause' },
};

/** The Standby button glyph (HS2-CSPKQX). */
export const STANDBY_ICON: GlanceIcon = { icon: Moon, name: 'moon' };
