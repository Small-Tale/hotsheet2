import { ToolbarText } from '../components/toolbar-text';

export function ToolbarTextDemo() {
  return <section class="toolbar-text-demo" aria-label="ToolbarText demo">
    <div><h2>Large</h2><ToolbarText text="Hot Sheet 2" size="large" /></div>
    <div><h2>Default</h2><ToolbarText text="All Tickets" /></div>
    <div><h2>Small</h2><ToolbarText text="HS2-C1TY0F" size="small" /></div>
  </section>;
}
