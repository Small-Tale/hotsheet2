import { Text } from '@kerfjs/ui/text';

/** Where a demo reports the side effect a control would have had in the real app (HS2-Z3FMEZ). */
export function DemoEvent({
  children,
  live = true,
  attributes,
}: {
  children?: Parameters<typeof Text>[0]['children'];
  /** Announce changes politely; static explanatory output opts out. */
  live?: boolean;
  /** Extra `data-*` hooks a demo's interaction wiring writes into. */
  attributes?: Readonly<Record<`data-${string}`, string>>;
}) {
  return (
    <Text
      variant="p"
      size="compact"
      tone="quiet"
      data-demo-event
      aria-live={live ? 'polite' : undefined}
      {...attributes}
    >
      {/* An empty report keeps its line so the first event does not shift the controls below it. */}
      {children === undefined || children === '' ? '\u00a0' : children}
    </Text>
  );
}
