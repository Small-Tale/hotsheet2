import { Badge } from '@kerfjs/ui/badge';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { ProgressBar } from '@kerfjs/ui/progress-bar';
import { Row } from '@kerfjs/ui/row';
import { SummaryRow } from '@kerfjs/ui/summary-row';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

const perTarget = [
  { target: 'Target A', runs: '3 runs · 2 finished', lead: 'Lead time 1 h 40 min', cost: 'Known cost $7.20' },
  { target: 'Target B', runs: '3 runs · 1 finished', lead: 'Lead time 2 h 05 min', cost: 'Cost unknown · remote' },
  {
    target: 'Target C',
    runs: '1 run · 0 finished',
    lead: 'Lead time unknown · unfinished',
    cost: 'Cost unknown · remote',
  },
];

/**
 * The Summary section (§26.21): totals, then one row per target. An unknown measure reads as
 * unknown with its reason, never as zero, and a cost total says when it is partial.
 */
export function OrchestrationMockSummary() {
  return (
    <List gap="l">
      <section aria-label="Totals">
        <List gap="xs">
          <ListHeader label="Totals" status="As of 14:09" />
          <ProgressBar label="Work items finished" value={3} max={7} valueText="3 of 7" showValue size="compact" />
          <ValueTable label="Totals" density="compact" stackBelow="narrow">
            <ValueTableRow label="Runs" value="7 · 3 finished, 3 active, 1 queued" />
            <ValueTableRow label="Lead time (median)" value="1 h 52 min · 3 known, 4 unknown (unfinished)" />
            <ValueTableRow label="Queue wait (median)" value="6 min · 6 known, 1 unknown (never dispatched)" />
            <ValueTableRow label="Blocked time" value="38 min · paused 0, taken over 11 min, unreachable 27 min" />
            <ValueTableRow label="Retries" value="2 validation re-runs, 1 replaced session" />
            <ValueTableRow label="CI" value="4 passed, 1 failed, 1 pending, 1 unverified" />
            <ValueTableRow label="Known AI cost" value="$7.20 · partial: 4 work items not read here" />
          </ValueTable>
        </List>
      </section>
      <section aria-label="By Target">
        <List gap="xs">
          <ListHeader label="By Target" />
          <List ariaLabel="Summary by Target">
            {perTarget.map((row, index) => (
              <SummaryRow
                semanticRole="group"
                ariaLabel={row.target}
                title={<Text variant="span">{row.target}</Text>}
                listEdge={index === 0 ? 'start' : index === perTarget.length - 1 ? 'end' : undefined}
                metadata={
                  <Row gap="xs" wrap vAlign="middle">
                    <Badge tone="neutral" size="compact">
                      {row.runs}
                    </Badge>
                    <Text tone="quiet" size="compact" flush>
                      {`${row.lead} · ${row.cost}`}
                    </Text>
                  </Row>
                }
              />
            ))}
          </List>
        </List>
      </section>
    </List>
  );
}
