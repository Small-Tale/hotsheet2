import './notification-card.css';

import { Badge } from '@kerfjs/ui/badge';
import { foregroundColorVar, pct } from '@kerfjs/ui/css-values';
import { LucideIcon, type LucideIconProps } from '@kerfjs/ui/lucide-icon';
import { Text } from '@kerfjs/ui/text';
import type { SafeHtml } from 'kerfjs/jsx-runtime';
import { Bot, type IconNode } from 'lucide';

/**
 * How the card sits on its surface: `popup` floats above content (loud border, large shadow),
 * `popup-contained` keeps the loud border while its container paints the shadow, `list` rests in a
 * notification list (quiet shadow), and `flat` is a settled history entry without shadow.
 */
export type NotificationCardElevation = 'popup' | 'popup-contained' | 'list' | 'flat';

/**
 * How the footer adapts when the card is narrow: `wrap` keeps quiet actions and the trailing action on
 * one row until they no longer fit, and `stack` gives each footer group its own full-width row.
 */
export type NotificationCardFooterLayout = 'wrap' | 'stack';

export interface NotificationCardDetails {
  text: string;
  /** `code` is the monospace command or diff block; `prose` keeps body type for a plain message. */
  appearance?: 'code' | 'prose';
  /** Cap a long block at a scrolling height instead of letting the card grow. */
  bounded?: boolean;
}

export interface NotificationCardProps {
  /** Agent or session that raised the notification. */
  identity: string;
  /** Optional role shown after the identity, such as the agent's ticket role. */
  role?: string;
  projectName: string;
  /** Relative time shown at the header's end. */
  time?: string;
  summary: { icon: IconNode; iconName: string; label: string; color: LucideIconProps['color'] };
  details?: NotificationCardDetails;
  /** Notification-specific content between the details and the footer. */
  children?: SafeHtml;
  /** Quiet secondary actions at the footer's start. */
  quietActions?: SafeHtml;
  /** Decisions or the primary action at the footer's end. */
  endActions?: SafeHtml;
  elevation?: NotificationCardElevation;
  footerLayout?: NotificationCardFooterLayout;
  rootAttributes?: Record<string, string | undefined>;
}

const SUCCESS_IDENTITY = foregroundColorVar('--wa-color-success-fill-loud');

/** One compact notification frame shared by permission requests and halted AI sessions (HS2-XS71FB). */
export function NotificationCard({
  identity,
  role,
  projectName,
  time,
  summary,
  details,
  children,
  quietActions,
  endActions,
  elevation = 'list',
  footerLayout = 'wrap',
  rootAttributes = {},
}: NotificationCardProps) {
  return (
    <article class="notification-card" data-elevation={elevation} data-footer-layout={footerLayout} {...rootAttributes}>
      <header class="notification-card__header">
        <span class="notification-card__identity">
          <span class="notification-card__identity-icon">
            <LucideIcon icon={Bot} name="bot" size="s" color={SUCCESS_IDENTITY} />
          </span>
          <Text variant="span" wrap="truncate" border="none" color={foregroundColorVar('--wa-color-neutral-on-normal')}>
            <strong>{identity}</strong>
          </Text>
          {role && <span aria-hidden="true">·</span>}
          {role && <span>{role}</span>}
        </span>
        {/* Kerf Badge truncates the project pill with a tooltip (KF-0WKZY2, HS2-B0B28R); the slot places it. */}
        <span class="notification-card__project">
          <Badge appearance="quiet" shape="pill" tone="neutral" maxWidth={pct(100)}>
            {projectName}
          </Badge>
        </span>
        {time && <time class="notification-card__time">{time}</time>}
      </header>
      <div class="notification-card__summary">
        <span class="notification-card__summary-icon">
          <LucideIcon icon={summary.icon} name={summary.iconName} size={17.6} color={summary.color} />
        </span>
        <strong>{summary.label}</strong>
      </div>
      {details && details.text.trim() && (
        <pre
          class="notification-card__details"
          data-appearance={details.appearance ?? 'code'}
          data-bounded={details.bounded ? 'true' : undefined}
        >
          <code>{details.text}</code>
        </pre>
      )}
      {children}
      {(quietActions || endActions) && (
        <footer class="notification-card__footer">
          {quietActions && <div class="notification-card__quiet-actions">{quietActions}</div>}
          {endActions && <div class="notification-card__end-actions">{endActions}</div>}
        </footer>
      )}
    </article>
  );
}
