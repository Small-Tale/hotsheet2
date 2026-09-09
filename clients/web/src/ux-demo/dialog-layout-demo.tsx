import { AppWindow, ServerCog } from 'lucide';

import { DialogHeader, ValueTable } from '../components/dialog-layout';
import { Hs1MigrationBanner, Hs1MigrationDialog } from '../components/hs1-migration';
import { LucideIcon } from '../components/lucide-icon';

export function DialogHeaderDemo(){return <section class="dialog-layout-demo dialog-surface"><DialogHeader title="Dialog title" titleId="dialog-header-demo-title" summary="A concise explanation of the current dialog state." icon={<LucideIcon icon={ServerCog} name="server-cog"/>}/></section>}
export function ValueTableDemo(){return <section class="dialog-layout-demo dialog-surface"><DialogHeader title="Value table" titleId="value-table-demo-title" summary="Static metadata uses inset separators and aligned values." icon={<LucideIcon icon={AppWindow} name="app-window"/>}/><div class="dialog-layout-demo__body"><ValueTable label="Example metadata"><div><dt>Version</dt><dd>0.1.0</dd></div><div><dt>Build</dt><dd><code>source-sha256:example</code></dd></div><div><dt>Protocol</dt><dd>1–1</dd></div></ValueTable></div></section>}
export function Hs1MigrationDialogDemo(){return <Hs1MigrationDialog projectName="Demo" projectRoot="/work/demo" sourcePath="/work/demo/.hotsheet" databasePath="/work/demo/.hotsheet/db" postgresVersion="17" defaultStore="/work/demo.hs2" open busy={false}/>}
export function Hs1MigrationBannerDemo(){return <section class="dialog-layout-demo dialog-surface"><Hs1MigrationBanner databasePath="/work/demo/.hotsheet/db"/></section>}
