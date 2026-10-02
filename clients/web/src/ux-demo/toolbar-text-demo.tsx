import { ToolbarText } from '@kerfjs/ui/toolbar-text';

export function ToolbarTextDemo() {
  return (
    <section class="toolbar-text-demo" aria-label="ToolbarText demo">
      <div>
        <h2 class="toolbar-text-demo__caption">Large</h2>
        <ToolbarText text="Hot Sheet 2" size="large" />
      </div>
      <div>
        <h2 class="toolbar-text-demo__caption">Default</h2>
        <ToolbarText text="Queue" />
      </div>
      <div>
        <h2 class="toolbar-text-demo__caption">Small</h2>
        <ToolbarText text="HS2-C1TY0F" size="small" />
      </div>
    </section>
  );
}
