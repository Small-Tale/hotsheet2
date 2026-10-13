import { CatalogExample } from '@kerfjs/ui/catalog';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';

import { ProviderIcon, type ProviderIconKind, type ProviderIconSize } from '../components/provider-icon';
import type { DemoModule } from './demo-module';

const PROVIDERS: readonly { kind: ProviderIconKind; label: string }[] = [
  { kind: 'github', label: 'GitHub' },
  { kind: 'gitlab', label: 'GitLab' },
  { kind: 'jira', label: 'Jira' },
];

const SIZES: readonly { size: ProviderIconSize; caption: string; guidance: string }[] = [
  {
    size: 'm',
    caption: 'size="m" (default)',
    guidance: 'Follows the surrounding font size (1em): source choices, list rows, and inline labels.',
  },
  {
    size: 'l',
    caption: 'size="l"',
    guidance: 'A fixed 24px identity mark: the Accounts card header.',
  },
];

/** Every public ProviderIcon kind at both size variants (HS2-PK8THJ). */
export function ProviderIconDemo() {
  return (
    <>
      {SIZES.map(({ size, caption, guidance }) => (
        <CatalogExample align="glyph" label={caption} note={guidance} rootAttributes={{ 'data-size-variant': size }}>
          <Row gap="l" wrap>
            {PROVIDERS.map(({ kind, label }) => (
              <Row gap="xs" vAlign="middle" rootAttributes={{ 'data-provider': kind }}>
                <ProviderIcon kind={kind} size={size} />
                <Text variant="span" aria-hidden="true">
                  {label}
                </Text>
              </Row>
            ))}
          </Row>
        </CatalogExample>
      ))}
    </>
  );
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'provider-icon': { catalogExamples: true, Demo: ProviderIconDemo },
  },
};
