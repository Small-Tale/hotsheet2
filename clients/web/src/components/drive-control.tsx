import { Play, Square } from 'lucide';
import { LucideIcon } from './lucide-icon';
import './drive-control.css';

export interface DriveControlProps { running: boolean; tool?: string }
export function DriveControl({ running, tool = 'AI tool' }: DriveControlProps) {
  return <button type="button" class="drive-control" data-component="drive-control" data-running={String(running)} data-action="toggle-drive" aria-label={running ? `Stop ${tool}` : `Start ${tool}`}>
    <LucideIcon icon={running ? Square : Play} name={running ? 'square' : 'play'} />
    <span>{running ? `${tool} running` : `Drive with ${tool}`}</span>
  </button>;
}
