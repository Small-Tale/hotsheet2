import './project-dialog-demo.css';

import { delegate, signal } from 'kerfjs';

import { ProjectDialog, RemoteProjectDialog } from '../components/project-dialog';

const variants = {
  projects: 'Remote projects',
  loading: 'Remote loading',
  empty: 'Remote empty',
  error: 'Remote error',
  local: 'Local project',
  recovery: 'Local recovery',
  busy: 'Recovery in progress',
} as const;
type ProjectDialogDemoVariant = keyof typeof variants;

const variant = signal<ProjectDialogDemoVariant>('projects'),
  open = signal(true),
  result = signal(''),
  localRoot = signal('/work/demo');

const checkouts = [
  { id: 'demo', root: '/work/demo', alias: 'Demo', stores: [] },
  {
    id: 'best-in-manila',
    root: '/work/projects/spotlight-cities/best-in-manila',
    alias: 'best-in-manila',
    stores: [],
  },
  {
    id: 'long-path',
    root: `/work/projects/${'a-very-long-project-folder-name-without-spaces-'.repeat(3)}checkout`,
    alias: 'Long project path',
    stores: [],
  },
  ...['code-a', 'code-b', 'domotion', 'hotsheet2', 'karwan', 'kerf', 'procurement'].map((name) => ({
    id: name,
    root: `/work/projects/${name}`,
    alias: name,
    stores: [],
  })),
];

export function ProjectDialogDemo() {
  const local = ['local', 'recovery', 'busy'].includes(variant.value);
  return (
    <section class="project-dialog-demo" aria-label="Project dialog variants">
      <div class="project-dialog-demo__variants">
        {Object.entries(variants).map(([id, label]) => (
          <wa-button data-action="show-project-dialog-demo" data-variant={id}>
            {label}
          </wa-button>
        ))}
      </div>
      <p role="status">{result.value}</p>
      <ProjectDialog
        open={open.value && local}
        root={localRoot.value}
        error={variant.value === 'recovery' ? 'The registered local server is not responding.' : ''}
        recovery={variant.value === 'recovery' || variant.value === 'busy' ? { expected: { pid: 4242 } } : undefined}
        recoveryBusy={variant.value === 'busy'}
      />
      <RemoteProjectDialog
        open={open.value && !local}
        checkouts={variant.value === 'empty' ? [] : checkouts}
        loading={variant.value === 'loading'}
        error={variant.value === 'error' ? 'Could not load the projects open on the Hot Sheet server. Try again.' : ''}
      />
    </section>
  );
}

/** Wire demo-only fixture effects while preserving the production dialog actions. */
export function wireProjectDialogDemo(root: HTMLElement) {
  const scope = '[aria-label="Project dialog variants"]';
  delegate(root, 'click', `${scope} [data-action="show-project-dialog-demo"]`, (_event, target) => {
    const next = (target as HTMLElement).dataset.variant;
    if (!next || !(next in variants)) return;
    variant.value = next as ProjectDialogDemoVariant;
    open.value = true;
  });
  delegate(root, 'click', `${scope} [data-action="open-remote-checkout"]`, (_event, target) => {
    result.value = `Opened ${(target as HTMLElement).dataset.checkoutRoot ?? ''}`;
    open.value = false;
  });
  delegate(
    root,
    'click',
    `${scope} [data-action="cancel-remote-project"], ${scope} [data-action="cancel-open-project"]`,
    () => {
      open.value = false;
    },
  );
  delegate(root, 'wa-hide', `${scope} wa-dialog`, () => {
    open.value = false;
  });
  delegate(root, 'input', `${scope} [name="project-root"]`, (_event, target) => {
    localRoot.value = (target as HTMLInputElement).value;
  });
  delegate(root, 'submit', `${scope} [data-action="open-project-form"]`, (event) => {
    event.preventDefault();
    result.value = `Opened ${localRoot.value}`;
    open.value = false;
  });
  delegate(root, 'click', `${scope} [data-action="browse-project-path"]`, () => {
    result.value = 'The demo uses fixture folders; production opens the native folder chooser.';
  });
  delegate(root, 'click', `${scope} [data-action="recover-unhealthy-server"]`, () => {
    result.value = 'Recovered the demo server.';
    variant.value = 'local';
  });
}
