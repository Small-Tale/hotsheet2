import './notification-inspector.css';

import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Toolbar } from '@kerfjs/ui/toolbar';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';
import { PanelRightClose } from 'lucide';

export function NotificationInspector() {
  return (
    <aside class="notification-inspector-empty" aria-label="Notification inspector">
      <Toolbar
        divider={false}
        trailing={
          <ToolbarControlGroup appearance="borderless" single>
            <button
              type="button"
              data-action="close-ticket-inspector"
              aria-label="Hide notification inspector"
              title="Hide notification inspector"
            >
              <LucideIcon icon={PanelRightClose} name="panel-right-close" />
            </button>
          </ToolbarControlGroup>
        }
      />
    </aside>
  );
}
