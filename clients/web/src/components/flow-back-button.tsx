import '@awesome.me/webawesome/dist/components/button/button.js';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { ChevronLeft } from 'lucide';

/**
 * The one back affordance across multi-step setup dialogs and flows (HS2-WJ4JDW): a plain brand Web
 * Awesome button, which owns its hover, focus, disabled, and cursor states. The Row keeps it
 * shrink-wrapped at the start of a stretching form column.
 */
export function FlowBackButton({
  action,
  label,
  disabled = false,
}: {
  action: string;
  label: string;
  disabled?: boolean;
}) {
  return (
    <Row>
      <wa-button
        class="flow-back-button"
        appearance="plain"
        variant="brand"
        size="small"
        type="button"
        data-action={action}
        disabled={disabled}
      >
        <LucideIcon slot="start" size="s" icon={ChevronLeft} name="chevron-left" />
        {label}
      </wa-button>
    </Row>
  );
}
