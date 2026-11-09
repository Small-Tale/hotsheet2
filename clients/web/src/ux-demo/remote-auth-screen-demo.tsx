import { CatalogExample } from '@kerfjs/ui/catalog';

import { RemoteAuthScreen } from '../components/remote-auth-screen';
import type { DemoModule } from './demo-module';

export const demoModule: DemoModule = {
  entries: {
    'remote-auth-screen': {
      catalogExamples: true,
      Demo: () => (
        <CatalogExample label="Remote Browser Pairing" viewport={{ width: 'full', height: 'tall' }}>
          <RemoteAuthScreen label="Synthetic browser" pairing={undefined} message="" busy={false} />
        </CatalogExample>
      ),
    },
  },
};
