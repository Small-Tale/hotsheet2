import { CatalogExample } from '@kerfjs/ui/catalog';
import { Pane } from '@kerfjs/ui/pane';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { Text } from '@kerfjs/ui/text';
import { delegate, signal } from 'kerfjs';
import { createScope } from 'kerfjs/scope';

import {
  ContentTransition,
  type ContentTransitionDirection,
  type ContentTransitionSide,
  type ContentTransitionStyle,
} from '../components/content-transition';
import { DEMO_ACTIONS } from './demo-actions';
import type { DemoModule, FormControl } from './demo-module';
import { syncSettingsControls } from './settings-controls';

export const transitionSide = signal<ContentTransitionSide>('a'),
  transitionStyle = signal<ContentTransitionStyle>('push'),
  transitionDirection = signal<ContentTransitionDirection>('forward');
const card = (title: string, copy: string) => (
  <article>
    <Text size="compact" tone="quiet">
      Ticket support
    </Text>
    <Text variant="h2" size="large">
      {title}
    </Text>
    <Text>{copy}</Text>
  </article>
);
export function ContentTransitionDemo() {
  return (
    <>
      <CatalogExample
        label="Setup Dialog"
        note="Navigation moves between the two sides; the settings pane picks the content transition style."
        viewport={{ width: 'medium', frame: 'solid', surface: 'default', shadow: true }}
      >
        <Pane
          chromeDividers="always"
          header={
            <Row textInsets="trbl">
              <ContentTransition
                active={transitionSide.value}
                style="crossfade"
                region="label"
                label="Setup title transition"
                a={'Set up ticket support' as never}
                b={'Connect repository' as never}
              />
            </Row>
          }
          footer={
            <Row controlInsets="trbl">
              <ContentTransition
                active={transitionSide.value}
                style="crossfade"
                region="footer"
                sideLayout="actions"
                label="Setup actions transition"
                a={<wa-button>Cancel</wa-button>}
                b={
                  <>
                    <wa-button>Skip</wa-button>
                    <wa-button appearance="accent">Connect</wa-button>
                  </>
                }
              />
            </Row>
          }
        >
          <ContentTransition
            active={transitionSide.value}
            style={transitionStyle.value}
            direction={transitionDirection.value}
            label="Setup navigation preview"
            a={card('Choose a source', 'Create a ticket repository or connect an issue provider.')}
            b={card('Connect repository', 'Enter a remote URL so tickets are backed up.')}
          />
        </Pane>
      </CatalogExample>
      <CatalogExample label="Navigation" align="inline-control" viewport={{ width: 'medium' }}>
        <Row hAlign="space-between" fill>
          <wa-button data-action="transition-back">Back</wa-button>
          <wa-button appearance="accent" data-action="transition-forward">
            Continue
          </wa-button>
        </Row>
      </CatalogExample>
    </>
  );
}
/** Restore the canonical transition settings and sync the live controls (HS2-X1SM48). */
export function resetContentTransitionDemo(root?: ParentNode): void {
  transitionStyle.value = 'push';
  transitionSide.value = 'a';
  transitionDirection.value = 'forward';
  if (root)
    syncSettingsControls(root, 'content-transition', {
      values: { 'transition-style': transitionStyle.value, 'transition-side': transitionSide.value },
    });
}

export function ContentTransitionSettings() {
  return (
    <form class="settings-form" data-settings="content-transition">
      <Select
        name="transition-style"
        label="Transition style"
        value={transitionStyle.value}
        choices={[
          { value: 'push', label: 'Push' },
          { value: 'crossfade', label: 'Crossfade' },
          { value: 'none', label: 'None' },
        ]}
      />
      <Select
        name="transition-side"
        label="Visible side"
        value={transitionSide.value}
        choices={[
          { value: 'a', label: 'A' },
          { value: 'b', label: 'B' },
        ]}
      />
      <wa-button type="button" data-action="reset-settings">
        Reset
      </wa-button>
    </form>
  );
}

/** Install this file's demo interactions on the catalog root (HS2-HMVBZW). */
function wireContentTransitionDemo(root: HTMLElement): () => void {
  const demoListeners = createScope();
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.transitionForward.selector, () => {
      transitionDirection.value = 'forward';
      transitionSide.value = 'b';
    }),
  );
  demoListeners.add(
    delegate(root, 'click', DEMO_ACTIONS.transitionBack.selector, () => {
      transitionDirection.value = 'backward';
      transitionSide.value = 'a';
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="content-transition"] [name="transition-style"]', (_event, target) => {
      transitionStyle.value = (target as FormControl).value as typeof transitionStyle.value;
    }),
  );
  demoListeners.add(
    delegate(root, 'change', '[data-settings="content-transition"] [name="transition-side"]', (_event, target) => {
      const side = (target as FormControl).value as typeof transitionSide.value;
      transitionDirection.value = side === 'b' ? 'forward' : 'backward';
      transitionSide.value = side;
    }),
  );
  return () => {
    demoListeners.dispose();
  };
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    'content-transition': {
      catalogExamples: true,
      Demo: ContentTransitionDemo,
      Settings: ContentTransitionSettings,
      reset: resetContentTransitionDemo,
    },
  },
  wire: wireContentTransitionDemo,
};
