import { CatalogExample } from '@kerfjs/ui/catalog';

import { announcerItem } from '../announcer-reel';
import { AnnouncerOverlay } from '../components/announcer-overlay';
import type { DemoModule } from './demo-module';

const item = announcerItem('demo', 'Demo project', 'demo-store', {
  id: 'demo-event',
  ts: '2026-11-01T12:00:00Z',
  tool: 'codex',
  kind: 'edit',
  summary: 'Updated the synthetic sample file',
  detail: { path: 'src/sample.ts', before: 'const count = 1;', after: 'const count = 2;' },
  importance: 'normal',
});

export const demoModule: DemoModule = {
  entries: {
    'announcer-overlay': {
      catalogExamples: true,
      Demo: () => (
        <CatalogExample label="Announcer Reel" viewport={{ width: 'full', height: 'tall' }}>
          <AnnouncerOverlay open expanded live={false} speaking={false} audible items={[item]} index={0} />
        </CatalogExample>
      ),
    },
  },
};
