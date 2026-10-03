import { Select } from '@kerfjs/ui/select';
import { signal } from 'kerfjs';

import {
  ContentTransition,
  type ContentTransitionDirection,
  type ContentTransitionSide,
  type ContentTransitionStyle,
} from '../components/content-transition';
import { syncSettingsControls } from './settings-controls';

export const transitionSide = signal<ContentTransitionSide>('a'),
  transitionStyle = signal<ContentTransitionStyle>('push'),
  transitionDirection = signal<ContentTransitionDirection>('forward');
const card = (title: string, copy: string) => (
  <article class="content-transition-demo__card">
    <small>Ticket support</small>
    <h2>{title}</h2>
    <p>{copy}</p>
  </article>
);
export function ContentTransitionDemo() {
  return (
    <section class="content-transition-demo">
      <div class="content-transition-demo__dialog">
        <header>
          <ContentTransition
            active={transitionSide.value}
            style="crossfade"
            region="label"
            label="Setup title transition"
            a={'Set up ticket support' as never}
            b={'Connect repository' as never}
          />
        </header>
        <ContentTransition
          active={transitionSide.value}
          style={transitionStyle.value}
          direction={transitionDirection.value}
          label="Setup navigation preview"
          a={card('Choose a source', 'Create a ticket repository or connect an issue provider.')}
          b={card('Connect repository', 'Enter a remote URL so tickets are backed up.')}
        />
        <footer>
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
        </footer>
      </div>
      <div class="content-transition-demo__actions">
        <wa-button data-action="transition-back">Back</wa-button>
        <wa-button appearance="accent" data-action="transition-forward">
          Continue
        </wa-button>
      </div>
    </section>
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
