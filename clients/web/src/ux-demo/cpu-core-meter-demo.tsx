import { CatalogExample, CatalogExampleStack } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';

import { CpuCoreMeter } from '../components/cpu-core-meter';
import type { DemoModule } from './demo-module';

const MIXED = [96, 81, 62, 44, 33, 21, 12, 6, 2, 0];
const MANY = Array.from({ length: 64 }, (_, index) => (index * 37) % 101);

/** Fixed per-core readings (HS2-CPFBWP). Hover the meter for each core's value. */
export function CpuCoreMeterDemo() {
  return (
    <CatalogExampleStack>
      <CatalogExample
        label="Ten Cores"
        note="Each column is one logical core, lit from the bottom in fifths; the top segment takes the warning tone at 80% or more."
      >
        <Row gap="m" vAlign="middle" wrap>
          {CpuCoreMeter({ cores: MIXED })}
          {CpuCoreMeter({ cores: new Array<number>(10).fill(0) })}
        </Row>
      </CatalogExample>
      <CatalogExample
        label="Many Cores"
        note="A machine with many cores narrows its columns to fit the card instead of widening it."
      >
        {CpuCoreMeter({ cores: MANY })}
      </CatalogExample>
    </CatalogExampleStack>
  );
}

export const demoModule: DemoModule = {
  entries: {
    'cpu-core-meter': { catalogExamples: true, Demo: CpuCoreMeterDemo },
  },
};
