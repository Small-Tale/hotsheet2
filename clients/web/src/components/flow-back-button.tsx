import { Button } from '@kerfjs/ui/button';
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
      <Button
        className={'flow-back-button'}
        emphasis="plain"
        tone={'brand'}
        size="compact"
        type="button"
        action={action}
        disabled={disabled}
        icon={<LucideIcon size="s" icon={ChevronLeft} name="chevron-left" />}
      >
        {label}
      </Button>
    </Row>
  );
}
