import './command-run-dialog.css';

import { Square, X } from 'lucide';

import type { CommandDefinition, CommandRun } from '../api';
import { LucideIcon } from './lucide-icon';

export function CommandRunDialog({ command, run, confirmStop = false }: { command?: CommandDefinition; run?: CommandRun; confirmStop?: boolean }) {
  if (!command) return null;
  if (confirmStop) return <dialog aria-label={`Stop ${command.title}?`} data-component="command-cancellation-dialog">
    <h2>Stop {command.title}?</h2>
    <p>The command is still running. Stop it now?</p>
    <footer class="command-run-dialog__actions"><button type="button" data-action="dismiss-command-dialog">Keep running</button><button type="button" class="command-run-dialog__stop" data-action="confirm-stop-command" data-run-id={run?.id}><LucideIcon icon={Square} name="square" /> Stop command</button></footer>
  </dialog>;
  return <dialog aria-label={command.title} data-component="command-run-dialog">
    <h2>{command.title}</h2>
    <header class="command-run-dialog__summary"><strong>{run ? run.state : 'Never run'}</strong>{run?.exit_code !== undefined && <span>Exit {run.exit_code}</span>}</header>
    <pre aria-label="Command output">{run?.output.length ? run.output.map(line => `${line.stream === 'stderr' ? 'error: ' : ''}${line.text}`).join('\n') : 'No output recorded.'}</pre>
    <footer class="command-run-dialog__actions"><button type="button" data-action="dismiss-command-dialog"><LucideIcon icon={X} name="x" /> Close</button></footer>
  </dialog>;
}
