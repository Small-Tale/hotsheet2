import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { ListHeader } from '@kerfjs/ui/list-header';
import { Row } from '@kerfjs/ui/row';
import { StateBanner } from '@kerfjs/ui/state-banner';

import type { MockScenario } from './fixtures';

export const ORCH_MOCK_ANSWER_APPROVAL = 'orch-mock-answer-approval';

interface Approval {
  id: string;
  title: string;
  detail: string;
}

const approvals = (scenario: MockScenario): Approval[] => [
  {
    id: 'approval-ta47',
    title: 'Start a Claude session on Target A for TA-47',
    detail: 'The policy asks before a session starts. Requested 14:06; lapses tomorrow at 14:06.',
  },
  ...(scenario === 'degraded'
    ? [
        {
          id: 'approval-tc11',
          title: 'Override readiness for TC-11',
          detail: 'Blocked because TA-44 CI failed. An override dispatches it anyway.',
        },
      ]
    : []),
];

/**
 * Decisions waiting on a person (§26.8): one banner per pending approval at the top of the view,
 * with Deny before Approve. Only a person can answer; an AI session sees them read-only.
 */
export function OrchestrationMockApprovals({ scenario }: { scenario: MockScenario }) {
  const pending = approvals(scenario);
  if (scenario === 'empty' || pending.length === 0) return <></>;
  return (
    <section aria-label="Waiting on You">
      <List gap="xs">
        <ListHeader label="Waiting on You" count={pending.length} countLabel={`${pending.length} decisions`} />
        {pending.map((approval) => (
          <StateBanner
            copyLayout="stacked"
            tone="warning"
            title={approval.title}
            detail={approval.detail}
            rootAttributes={{ 'data-approval-id': approval.id }}
            action={
              <Row gap="xs" wrap>
                <Button
                  size="compact"
                  shape="pill"
                  action={ORCH_MOCK_ANSWER_APPROVAL}
                  rootAttributes={{ 'data-answer': 'deny', 'data-approval-id': approval.id }}
                >
                  Deny
                </Button>
                <Button
                  size="compact"
                  shape="pill"
                  tone="brand"
                  action={ORCH_MOCK_ANSWER_APPROVAL}
                  rootAttributes={{ 'data-answer': 'approve', 'data-approval-id': approval.id }}
                >
                  Approve
                </Button>
              </Row>
            }
          />
        ))}
      </List>
    </section>
  );
}
