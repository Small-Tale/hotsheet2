import { Button } from '@kerfjs/ui/button';
import { ContentItem } from '@kerfjs/ui/content-item';
import { px, remify } from '@kerfjs/ui/css-values';
import { Grid } from '@kerfjs/ui/grid';
import { List } from '@kerfjs/ui/list';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Spacer } from '@kerfjs/ui/spacer';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import { awayClock, type AwayDigest, awayEventRow, formatAwayDuration } from '../away-digest';
import { SHELL_AND_GLOBAL_ACTIONS as actions } from '../interaction-attrs/shell-and-global';
import { formatGlanceValue } from './glance-gauge-geometry';

export interface AwayDigestDialogProps {
  digest?: AwayDigest;
  /** Formats each time; the catalog passes a fixed locale. */
  clock?: (ms: number) => string;
}

/**
 * While you were away (HS2-JJ9RYN, docs/34-glance-surfaces.md §34.9, mock `m7-away-digest-light`): the
 * standby window, three tiles, one row per event with the action that resolves it, Dismiss, and "Show
 * what needs me" while something does.
 */
export function AwayDigestDialog({ digest, clock = awayClock }: AwayDigestDialogProps) {
  if (!digest) return null;
  const range = `${clock(digest.enteredAtMs)} – ${clock(digest.exitedAtMs)} · ${formatAwayDuration(
    digest.exitedAtMs - digest.enteredAtMs,
  )} in standby`;
  return (
    <DialogSurface
      label="While you were away"
      describedBy="away-digest-range"
      preferredWidth={remify(640)}
      viewportGutter={remify(16)}
      rootAttributes={{ 'data-key': 'away-digest-dialog', 'data-away-digest': 'true', 'data-controlled-open': 'true' }}
      footer={
        <Row vAlign="middle" gap="xs">
          <Spacer flex />
          <Button emphasis="secondary" type="button" action={actions.dismissAwayDigest.value}>
            Dismiss
          </Button>
          {digest.needYou > 0 ? (
            <Button emphasis="primary" tone="brand" type="button" action={actions.showWhatNeedsMe.value}>
              Show What Needs Me
            </Button>
          ) : null}
        </Row>
      }
    >
      <List gap="l">
        <ListInsetText sides="rl">
          <Text variant="p" tone="quiet" flush id="away-digest-range">
            {range}
          </Text>
        </ListInsetText>
        <Grid minColumnWidth={px(104)} gap="m">
          <ContentItem appearance="surface" frame="framed" rootAttributes={{ 'data-away-tile': 'completed' }}>
            <List gap="2xs">
              <Text variant="span" size="xlarge" weight="strong" flush>
                {String(digest.completed)}
              </Text>
              <Text variant="span" size="compact" tone="quiet" flush>
                Completed
              </Text>
            </List>
          </ContentItem>
          <ContentItem
            appearance={digest.needYou > 0 ? 'warning' : 'surface'}
            frame="framed"
            rootAttributes={{ 'data-away-tile': 'need-you', 'data-attention': String(digest.needYou > 0) }}
          >
            <List gap="2xs">
              <Text variant="span" size="xlarge" weight="strong" flush>
                {String(digest.needYou)}
              </Text>
              <Text variant="span" size="compact" tone={digest.needYou > 0 ? 'default' : 'quiet'} flush>
                Need you
              </Text>
            </List>
          </ContentItem>
          <ContentItem appearance="surface" frame="framed" rootAttributes={{ 'data-away-tile': 'tokens' }}>
            <List gap="2xs">
              <Text variant="span" size="xlarge" weight="strong" flush>
                {formatGlanceValue(digest.tokens)}
              </Text>
              <Text variant="span" size="compact" tone="quiet" flush>
                Tokens
              </Text>
            </List>
          </ContentItem>
        </Grid>
        {digest.events.length ? (
          <List gap="xs" rootAttributes={{ 'data-away-events': '' }}>
            {digest.events.map((event) => {
              const row = awayEventRow(event, clock);
              return (
                <ListInsetText sides="rl" rootAttributes={{ 'data-away-event': event.kind }}>
                  <Row vAlign="middle" gap="xs">
                    <LucideIcon icon={row.icon.icon} name={row.icon.name} />
                    <Text variant="span" flush>
                      {row.subject ? <strong>{row.subject}</strong> : null}
                      {row.subject ? ` ${row.text}` : row.text}
                    </Text>
                    <Spacer flex />
                    {row.action ? (
                      <Button
                        emphasis="link"
                        type="button"
                        action={actions.openAwayEvent.value}
                        rootAttributes={{
                          'data-project-id': row.action.projectId,
                          'data-ticket-slug': row.action.slug,
                        }}
                      >
                        {row.action.label}
                      </Button>
                    ) : null}
                  </Row>
                </ListInsetText>
              );
            })}
          </List>
        ) : null}
      </List>
    </DialogSurface>
  );
}
