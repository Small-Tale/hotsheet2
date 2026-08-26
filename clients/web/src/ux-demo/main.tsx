import { delegate, mount, signal } from 'kerfjs';
import '@awesome.me/webawesome/dist/styles/webawesome.css';
import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/checkbox/checkbox.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import '@awesome.me/webawesome/dist/components/option/option.js';
import '@awesome.me/webawesome/dist/components/select/select.js';
import '@awesome.me/webawesome/dist/components/tag/tag.js';
import './style.css';
import { demoCatalog, findDemo, type DemoCategory, type DemoDefinition } from './catalog';
import { resetTagChipDemo, TagChipDemo, TagChipSettings, tagChipSettings } from './tag-chip-demo';

type FormControl = HTMLElement & { checked: boolean; value: string };
const defaultDemo = 'tag-chip';
const fromUrl = () => new URL(location.href).searchParams.get('component') ?? defaultDemo;
const selectedId = signal(findDemo(fromUrl())?.id ?? defaultDemo);
const settingsOpen = signal(false);

function demoLink(item: DemoDefinition) {
  const selected = item.id === selectedId.value;
  return <li><a href={`/ux-demo?component=${encodeURIComponent(item.id)}`} data-demo-id={item.id} aria-current={selected ? 'page' : undefined}><span>{item.name}</span><small>{item.implemented ? 'Demo' : item.phase.replace('-', ' ')}</small></a></li>;
}

function demoNavigation(category: DemoCategory) {
  return <section class="catalog-group"><h2>{category.name}</h2>{category.demos && <ul>{category.demos.map(demoLink)}</ul>}{category.children?.map(child => <section class="catalog-subgroup"><h3>{child.name}</h3><ul>{child.demos?.map(demoLink)}</ul></section>)}</section>;
}

function demoContent(item: DemoDefinition) {
  if (item.id === 'tag-chip') return <TagChipDemo />;
  return <section class="planned-demo" aria-label={`${item.name} planned demo`}><span>Planned component</span><p>The catalog entry and navigation are ready. Its real component demo will be added in a later slice.</p></section>;
}

function DemoApp() {
  const selected = findDemo(selectedId.value) ?? findDemo(defaultDemo)!;
  return (
    <main class={settingsOpen.value ? 'demo-shell demo-shell--settings-open' : 'demo-shell'}>
      <aside class="demo-master" aria-label="Component catalog">
        <header><p class="eyebrow">Hot Sheet</p><h1>UX components</h1><p>Production components with deterministic development support.</p></header>
        <nav>{demoCatalog.map(demoNavigation)}</nav>
      </aside>
      <article class="demo-detail">
        <header class="demo-detail__header"><div><p class="eyebrow">{selected.phase.replace('-', ' ')}</p><h1>{selected.name}</h1><p>{selected.description}</p></div>{selected.id === 'tag-chip' && <wa-button data-action="show-settings" aria-expanded={settingsOpen.value ? 'true' : 'false'}>Settings</wa-button>}</header>
        {demoContent(selected)}
      </article>
      {settingsOpen.value && <aside class="settings-inspector" aria-label={`${selected.name} settings`}>
        <header><div><p class="eyebrow">Demo settings</p><h2>{selected.name}</h2></div><wa-button size="small" data-action="close-settings">Close settings</wa-button></header>
        {selected.id === 'tag-chip' ? <TagChipSettings /> : <p>This demo has no adjustable settings.</p>}
      </aside>}
    </main>
  );
}

const root = document.querySelector<HTMLElement>('#ux-demo')!;
mount(root, DemoApp);

function selectDemo(id: string, push = true): void {
  if (!findDemo(id)) return;
  selectedId.value = id;
  if (push) history.pushState(null, '', `/ux-demo?component=${encodeURIComponent(id)}`);
}

delegate(root, 'click', '[data-demo-id]', (event, target) => { event.preventDefault(); selectDemo((target as HTMLElement).dataset.demoId!); });
delegate(root, 'click', '[data-action="show-settings"]', () => { settingsOpen.value = true; });
delegate(root, 'click', '[data-action="close-settings"]', () => { settingsOpen.value = false; });
delegate(root, 'click', '[data-action="reset-settings"]', () => resetTagChipDemo());
delegate(root, 'input', '[data-settings="tag-chip"] [name="label"]', (_event, target) => { tagChipSettings.label.value = (target as FormControl).value; });
delegate(root, 'change', '[data-settings="tag-chip"] [name]', (_event, target) => {
  const control = target as FormControl;
  switch (control.getAttribute('name')) {
    case 'variant': tagChipSettings.variant.value = control.value as typeof tagChipSettings.variant.value; break;
    case 'appearance': tagChipSettings.appearance.value = control.value as typeof tagChipSettings.appearance.value; break;
    case 'size': tagChipSettings.size.value = control.value as typeof tagChipSettings.size.value; break;
    case 'removable': tagChipSettings.removable.value = control.checked; break;
    case 'pill': tagChipSettings.pill.value = control.checked; break;
    case 'disabled': tagChipSettings.disabled.value = control.checked; break;
  }
});
delegate(root, 'wa-remove', '[data-component="tag-chip"]', (_event, target) => {
  if ((target as HTMLElement).dataset.disabled !== 'true') tagChipSettings.event.value = `Remove requested for ${(target as HTMLElement).dataset.tagId}`;
});
addEventListener('popstate', () => selectDemo(fromUrl(), false));
