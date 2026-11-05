import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';

import { GlanceSparkline } from '../components/glance-sparkline';
import type { DemoModule } from './demo-module';

const UPDATES = [1, 0, 2, 1, 0, 1, 2, 1, 1, 1, 2, 2];

/** Fixed series in each tone (HS2-ERTNJ7). Hover a line for its values. */
export function GlanceSparklineDemo() {
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Tones"
        note="Green is tickets updated; neutral is the calm default; warning marks a problem series."
      >
        <Row gap="m" vAlign="middle" wrap>
          {GlanceSparkline({ values: UPDATES, label: 'Tickets updated per 5 minutes', tone: 'success' })}
          {GlanceSparkline({ values: UPDATES, label: 'Neutral series' })}
          {GlanceSparkline({ values: UPDATES, label: 'Brand series', tone: 'brand' })}
          {GlanceSparkline({ values: [4, 3, 1, 0, 0, 0], label: 'Warning series', tone: 'warning' })}
        </Row>
      </CatalogExample>
      <CatalogExample label="Idle" note="An all-zero series draws a flat baseline, never a guessed trend.">
        {GlanceSparkline({ values: new Array<number>(12).fill(0), label: 'Tickets updated per 5 minutes' })}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'glance-sparkline': { catalogExamples: true, Demo: GlanceSparklineDemo },
  },
};
