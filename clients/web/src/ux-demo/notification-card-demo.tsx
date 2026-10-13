import { CatalogExample } from '@kerfjs/ui/catalog';
import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { attr, delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';
import { CircleAlert, ShieldQuestion } from 'lucide';

import {
  NotificationCard,
  type NotificationCardElevation,
  type NotificationCardFooterLayout,
} from '../components/notification-card';
import { DemoEvent } from './components/demo-event';
import type { DemoModule } from './demo-module';

/** A consumer-supplied footer action; NotificationCard itself renders no actions (HS2-V2G3W1). */
const DEMO_CARD_ACTION = attr('data-demo-notification-action');
export const notificationCardDemoResult = signal('Footer actions belong to the composing notification.');

const PERMISSION_SUMMARY = {
    icon: ShieldQuestion,
    iconName: 'shield-question',
    label: 'Run a shell command',
    color: foregroundColorVar('--wa-color-warning-fill-loud'),
  },
  HALT_SUMMARY = {
    icon: CircleAlert,
    iconName: 'circle-alert',
    label: 'Session halted',
    color: foregroundColorVar('--wa-color-danger-fill-loud'),
  };

function DemoAction({ label, appearance = 'plain' }: { label: string; appearance?: 'plain' | 'accent' | 'outlined' }) {
  return (
    <wa-button size="small" appearance={appearance} type="button" {...DEMO_CARD_ACTION(label)}>
      {label}
    </wa-button>
  );
}

function DemoCard({
  elevation,
  footerLayout,
}: {
  elevation: NotificationCardElevation;
  footerLayout: NotificationCardFooterLayout;
}) {
  return (
    <NotificationCard
      identity="Claude"
      role="worker"
      projectName="Hot Sheet demo"
      time="2m ago"
      summary={PERMISSION_SUMMARY}
      details={{ text: 'npm run test:e2e -- tests/ux-demo.spec.ts' }}
      elevation={elevation}
      footerLayout={footerLayout}
      quietActions={<DemoAction label="Always allow" />}
      endActions={
        <Row gap="xs" hAlign="right">
          <DemoAction label="Deny" appearance="outlined" />
          <DemoAction label="Allow" appearance="accent" />
        </Row>
      }
    />
  );
}

export function NotificationCardDemo() {
  return (
    <>
      <DemoEvent>{notificationCardDemoResult.value}</DemoEvent>
      {(['popup', 'list', 'flat'] as const).map((elevation) => (
        <CatalogExample label={`Elevation: ${elevation}`} viewport={{ width: 'wide' }}>
          <DemoCard elevation={elevation} footerLayout="wrap" />
        </CatalogExample>
      ))}
      {/* The stacked footer gives each action group its own full-width row on narrow surfaces. */}
      <CatalogExample label="Footer layout: stack" viewport={{ width: 'compact' }}>
        <DemoCard elevation="list" footerLayout="stack" />
      </CatalogExample>
      <CatalogExample label="Details: prose, bounded code, and custom content">
        <List gap="m">
          <NotificationCard
            identity="Codex"
            projectName="Hot Sheet demo"
            time="just now"
            summary={HALT_SUMMARY}
            details={{ text: 'Selected model is at capacity. Please try a different model.', appearance: 'prose' }}
            endActions={<DemoAction label="Open session" appearance="accent" />}
          />
          <NotificationCard
            identity="Claude"
            role="reviewer"
            projectName="A project with a very long name that truncates in its pill"
            summary={PERMISSION_SUMMARY}
            details={{
              text: Array.from({ length: 24 }, (_, line) => `+ line ${line + 1} of a long proposed diff`).join('\n'),
              bounded: true,
            }}
            elevation="flat"
          >
            <DemoEvent>Notification-specific content renders between the details and the footer.</DemoEvent>
          </NotificationCard>
        </List>
      </CatalogExample>
    </>
  );
}

/** Install this file's demo interactions on the catalog root. */
function wireNotificationCardDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'click', '[data-demo-notification-action]', (_event, target) => {
      notificationCardDemoResult.value = `${(target as HTMLElement).dataset.demoNotificationAction ?? ''} was chosen; the composing notification owns its decision.`;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The NotificationCard catalog entry: every elevation, both footer layouts, and each details appearance (HS2-V2G3W1). */
export const demoModule: DemoModule = {
  entries: {
    'notification-card': { catalogExamples: true, Demo: NotificationCardDemo },
  },
  wire: wireNotificationCardDemo,
};
