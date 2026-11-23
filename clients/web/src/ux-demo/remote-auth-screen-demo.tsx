import { CatalogExample } from '@kerfjs/ui/catalog';

import { RemoteAuthScreen } from '../components/remote-auth-screen';
import type { DemoModule } from './demo-module';

export const demoModule: DemoModule = {
  entries: {
    'remote-auth-screen': {
      catalogExamples: true,
      Demo: () => (
        <>
          <CatalogExample label="Remote Browser Pairing" viewport={{ width: 'full', height: 'tall' }}>
            <RemoteAuthScreen label="Synthetic browser" pairing={undefined} message="" busy={false} />
          </CatalogExample>
          {/* HS2-4B0KSD: each reason a session stopped working has its own screen. */}
          {(['expired', 'renewal-requested', 'revoked', 'unreachable'] as const).map((mode) => (
            <CatalogExample label={`Session ${mode}`} viewport={{ width: 'full', height: 'tall' }}>
              <RemoteAuthScreen label="Synthetic browser" pairing={undefined} message="" busy={false} mode={mode} />
            </CatalogExample>
          ))}
        </>
      ),
    },
  },
};
