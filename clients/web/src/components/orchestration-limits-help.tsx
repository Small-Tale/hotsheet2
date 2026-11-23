import { List } from '@kerfjs/ui/list';
import { Text } from '@kerfjs/ui/text';
import { ValueTable, ValueTableRow } from '@kerfjs/ui/value-table';

import {
  BUILD_AHEAD_HINT,
  MODELS_HINT,
  REQUIRED_RESOURCES_HINT,
  SESSIONS_HERE_HINT,
} from '../orchestration-settings-model';

/**
 * HS2-Y737GM: the Limits header's help popup: what each of a project's limits means, in the words
 * its field's hint uses. Read-only; Kerf's AnchoredSurface owns its chrome, position and dismissal.
 */
export function OrchestrationLimitsHelp() {
  return (
    <div data-component="orchestration-limits-help">
      {/* A readable measure, each meaning under its limit's name, at every width. */}
      <List gap="m" contentWidth="compact" contentAlign="start">
        <Text flush size="compact">
          Every limit starts Auto, so the orchestrator decides. Change one only to hold this project to something else;
          the rarely needed ones are under Advanced.
        </Text>
        <ValueTable label="About limits" density="compact" presentation="stacked">
          <ValueTableRow
            label="AI Tools Here"
            value="The AI tools sessions here may use. None checked is Auto: any tool installed and signed in where the project runs."
          />
          <ValueTableRow label="Models" value={MODELS_HINT} />
          <ValueTableRow
            label="Coordination Sessions"
            value="A dispatcher that hands ready work to workers and an integrator that ships handed-off work. Off by default."
          />
          <ValueTableRow label="Sessions Here" value={SESSIONS_HERE_HINT} />
          <ValueTableRow label="Build Ahead" value={BUILD_AHEAD_HINT} />
          <ValueTableRow label="Required Resources" value={REQUIRED_RESOURCES_HINT} />
          <ValueTableRow
            label="Machine Limits"
            value="Off is Auto: the policy’s Sessions per Machine and memory, CPU and disk floors apply."
          />
        </ValueTable>
      </List>
    </div>
  );
}
