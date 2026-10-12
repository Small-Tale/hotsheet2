import './content-transition.css';

import { Row } from '@kerfjs/ui/row';
import type { SafeHtml } from 'kerfjs/jsx-runtime';

export type ContentTransitionSide = 'a' | 'b';
export type ContentTransitionStyle = 'none' | 'crossfade' | 'push';
export type ContentTransitionDirection = 'forward' | 'backward';
/** How each side lays out its children: `block` (normal flow) or `actions` (an end-aligned, wrapping action row). */
export type ContentTransitionSideLayout = 'block' | 'actions';

export function ContentTransition({
  active,
  style = 'push',
  direction = 'forward',
  a,
  b,
  label = 'Changing content',
  region = 'content',
  sideLayout = 'block',
}: {
  active: ContentTransitionSide;
  style?: ContentTransitionStyle;
  direction?: ContentTransitionDirection;
  a: SafeHtml;
  b: SafeHtml;
  label?: string;
  region?: 'content' | 'label' | 'footer';
  sideLayout?: ContentTransitionSideLayout;
}) {
  // Action-row sides are a Kerf Row: end-aligned buttons that wrap at narrow widths (HS2-29Q3XG, HS2-5A766Z).
  const layout = (content: SafeHtml) =>
    sideLayout === 'actions' ? (
      <Row hAlign="right" vAlign="middle" gap="xs" wrap>
        {content}
      </Row>
    ) : (
      content
    );
  return (
    <section
      class="content-transition"
      data-component="content-transition"
      data-active-side={active}
      data-transition-style={style}
      data-transition-direction={direction}
      data-transition-region={region}
      data-side-layout={sideLayout}
      slot={region === 'content' ? undefined : region}
      aria-label={label}
    >
      <div
        class="content-transition__side"
        data-side="a"
        data-active={String(active === 'a')}
        aria-hidden={active === 'a' ? undefined : 'true'}
        inert={active === 'a' ? undefined : true}
      >
        {layout(a)}
      </div>
      <div
        class="content-transition__side"
        data-side="b"
        data-active={String(active === 'b')}
        aria-hidden={active === 'b' ? undefined : 'true'}
        inert={active === 'b' ? undefined : true}
      >
        {layout(b)}
      </div>
    </section>
  );
}
