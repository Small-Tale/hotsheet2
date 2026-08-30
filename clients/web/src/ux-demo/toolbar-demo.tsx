import { PanelLeftOpen, PanelRightOpen } from 'lucide';

import { LucideIcon } from '../components/lucide-icon';
import { Toolbar } from '../components/toolbar';
import { ToolbarControlGroup } from '../components/toolbar-control-group';
import { ToolbarText } from '../components/toolbar-text';

export function ToolbarDemo() {
  return <section class="toolbar-demo" aria-label="Toolbar demo">
    <Toolbar
      label="Example three-slot toolbar"
      leading={<ToolbarControlGroup appearance="borderless" single><button type="button" aria-label="Show left sidebar"><LucideIcon icon={PanelLeftOpen} name="panel-left-open" /></button></ToolbarControlGroup>}
      center={<ToolbarText text="Hot Sheet 2" size="large" />}
      trailing={<ToolbarControlGroup appearance="borderless" single><button type="button" aria-label="Show right sidebar"><LucideIcon icon={PanelRightOpen} name="panel-right-open" /></button></ToolbarControlGroup>}
    />
    <Toolbar label="Toolbar without divider" divider={false} center={<ToolbarText text="HS2-C1TY0F" size="small" />} />
  </section>;
}
