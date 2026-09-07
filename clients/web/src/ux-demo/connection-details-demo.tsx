import { signal } from 'kerfjs';

import type { CompatibilityAssessment } from '../compatibility';
import { ConnectionDetailsDialog } from '../components/connection-details-dialog';
import { syncSettingsControls } from './settings-controls';

export type ConnectionDetailsScenario='source-stale'|'revision-mismatch'|'server-too-old-safe'|'server-too-old-manual'|'client-too-old'|'unknown';
export const connectionDetailsScenario=signal<ConnectionDetailsScenario>('source-stale');

const server={generation:'hs2',application_version:'0.1.0',build_revision:'source-sha256:old',source_revision:'source-sha256:current',source_stale:true,protocol:{min:1,max:1},started_at:'2026-09-07T01:42:10Z'} as const;

export function connectionDetailsAssessment(scenario:ConnectionDetailsScenario):CompatibilityAssessment {
  if(scenario==='unknown')return{kind:'unknown',detail:'The server did not provide compatibility metadata.',revisionMismatch:false,sourceStale:false,canRestartServer:false,clientProtocol:{min:1,max:1},clientRevision:'source-sha256:client'};
  if(scenario==='client-too-old')return{kind:'client_too_old',detail:'Client protocol 1–1 is older than server protocol 2–2.',revisionMismatch:true,sourceStale:false,canRestartServer:false,clientProtocol:{min:1,max:1},clientRevision:'source-sha256:client',server:{...server,source_stale:false,source_revision:server.build_revision,protocol:{min:2,max:2}}};
  if(scenario.startsWith('server-too-old'))return{kind:'server_too_old',detail:'Server protocol 0–0 is older than client protocol 1–1.',revisionMismatch:true,sourceStale:false,canRestartServer:scenario==='server-too-old-safe',clientProtocol:{min:1,max:1},clientRevision:'source-sha256:client',server:{...server,source_stale:false,source_revision:server.build_revision,protocol:{min:0,max:0}}};
  if(scenario==='revision-mismatch')return{kind:'compatible',detail:'The running server build differs from this development checkout.',revisionMismatch:true,sourceStale:false,canRestartServer:false,clientProtocol:{min:1,max:1},clientRevision:'source-sha256:client',server:{...server,source_stale:false,source_revision:server.build_revision}};
  return{kind:'compatible',detail:'The running server build differs from this checkout.',revisionMismatch:true,sourceStale:true,canRestartServer:false,clientProtocol:{min:1,max:1},clientRevision:'source-sha256:client',server};
}

export function resetConnectionDetailsDemo(root?:ParentNode){connectionDetailsScenario.value='source-stale';if(root)syncSettingsControls(root,'connection-details-dialog',{values:{scenario:'source-stale'}})}
export function ConnectionDetailsDialogDemo(){return <ConnectionDetailsDialog embedded assessment={connectionDetailsAssessment(connectionDetailsScenario.value)}/>}
export function ConnectionDetailsDialogSettings(){return <form class="settings-form" data-settings="connection-details-dialog"><wa-select name="scenario" label="Compatibility scenario" value={connectionDetailsScenario.value}>{([
  ['source-stale','Source build is stale'],['revision-mismatch','Detached compatible build'],['server-too-old-safe','Server too old, safe restart'],['server-too-old-manual','Server too old, manual restart'],['client-too-old','Client too old'],['unknown','Metadata unavailable'],
] as const).map(([value,label])=><wa-option value={value}>{label}</wa-option>)}</wa-select><wa-button type="button" data-action="reset-settings">Reset</wa-button></form>}
