import './connection-details-dialog.css';

import { ServerCog } from 'lucide';

import type { CompatibilityAssessment, CompatibilityRange } from '../compatibility';
import { DialogHeader, ValueTable } from './dialog-layout';
import { LucideIcon } from './lucide-icon';

function range(value: CompatibilityRange | undefined): string {
  return value ? `${value.min}–${value.max}` : 'Not reported';
}

function valueOrUnavailable(value: string | null | undefined): string {
  return value || 'Not reported';
}

export function connectionRecoveryGuidance(assessment: CompatibilityAssessment): string {
  if (assessment.sourceStale) return 'Run cargo build -p hotsheet-server, stop the older running Hot Sheet server, then reopen this project. Your project data remains usable because the protocol ranges overlap.';
  if (assessment.kind === 'server_too_old') return assessment.canRestartServer
    ? 'Update the server build, then use a quiescence-safe restart before reopening the project.'
    : 'Update or rebuild the Hot Sheet server, stop the old server manually after its active work finishes, then reopen the project. Automatic restart is unavailable because this server cannot guarantee quiescence.';
  if (assessment.kind === 'client_too_old') return 'Update this Hot Sheet client, then reload it. The newer server cannot safely serve this client protocol.';
  if (assessment.kind === 'unknown') return 'Restart the Hot Sheet server and reopen the project. If metadata is still unavailable, inspect the server log and confirm that both client and server are HS2 builds.';
  if (assessment.revisionMismatch) return 'If this detached build is intentional, you can continue: the protocol ranges overlap. Otherwise stop the running server, rebuild it from the current checkout, and reopen this project.';
  return 'Reconnect after updating the incompatible Hot Sheet component.';
}

export function ConnectionDetailsDialog({ assessment,embedded=false }: { assessment?: CompatibilityAssessment;embedded?:boolean }) {
  if (!assessment) return null;
  const server = assessment.server;
  const started = server?.started_at ? new Date(server.started_at) : undefined;
  const startedLabel = started && !Number.isNaN(started.valueOf()) ? started.toLocaleString() : valueOrUnavailable(server?.started_at);
  const summary=assessment.detail || (assessment.revisionMismatch ? 'The running server build differs from this development checkout.' : 'Hot Sheet could not confirm compatible build metadata.');
  return <section popover={embedded?undefined:'auto'} id={embedded?undefined:'connection-details-dialog'} class="dialog-surface connection-details-dialog" data-component="connection-details-dialog" data-kind={assessment.kind} data-embedded={embedded?'true':undefined} role="dialog" aria-labelledby="connection-details-title" aria-describedby="connection-details-summary">
    <DialogHeader title="Server build details" titleId="connection-details-title" summary={summary} summaryId="connection-details-summary" icon={<LucideIcon icon={ServerCog} name="server-cog"/>}/>
    <div class="connection-details-dialog__body">
      <ValueTable className="connection-details-dialog__metadata" label="Client and server build metadata">
        <div><dt>Running server version</dt><dd>{valueOrUnavailable(server?.application_version)}</dd></div>
        <div><dt>Running server build</dt><dd><code>{valueOrUnavailable(server?.build_revision)}</code></dd></div>
        <div><dt>Current server source</dt><dd><code>{valueOrUnavailable(server?.source_revision)}</code></dd></div>
        <div><dt>Client build</dt><dd><code>{valueOrUnavailable(assessment.clientRevision)}</code></dd></div>
        <div><dt>Protocol ranges</dt><dd>Client {range(assessment.clientProtocol)} · Server {range(server?.protocol)}</dd></div>
        <div><dt>Server started</dt><dd>{startedLabel}</dd></div>
      </ValueTable>
      <section><h3>What to do</h3><p class="connection-details-dialog__guidance">{connectionRecoveryGuidance(assessment)}</p></section>
    </div>
  </section>;
}
