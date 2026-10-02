import { ProviderIcon, type ProviderIconKind, type ProviderIconSize } from '../components/provider-icon';

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
    <section class="component-stage provider-icon-demo" aria-label="ProviderIcon demo">
      {SIZES.map(({ size, caption, guidance }) => (
        <figure class="provider-icon-demo__variant" data-size-variant={size}>
          <figcaption>{caption}</figcaption>
          <div class="component-stage__canvas">
            <ul class="provider-icon-demo__providers">
              {PROVIDERS.map(({ kind, label }) => (
                <li class="provider-icon-demo__provider" data-provider={kind}>
                  <ProviderIcon kind={kind} size={size} />
                  <span aria-hidden="true">{label}</span>
                </li>
              ))}
            </ul>
          </div>
          <p class="component-stage__guidance">{guidance}</p>
        </figure>
      ))}
    </section>
  );
}
