import { ChevronDown, Hammer, Send, TestTube2, type IconNode } from 'lucide';
import { LucideIcon } from './lucide-icon';
import { customizationContrastColor, resolveCustomizationColor } from './customization-palette';
import './command-navigation.css';

export interface CommandNavigationItem { id: string; label: string; color: string; icon: 'send' | 'test' | 'build'; running?: boolean }
export interface CommandNavigationProps { label: string; commands: CommandNavigationItem[]; expanded: boolean }
const icons: Record<CommandNavigationItem['icon'], [IconNode, string]> = { send: [Send, 'send'], test: [TestTube2, 'test-tube-2'], build: [Hammer, 'hammer'] };
export function CommandNavigation({ label, commands, expanded }: CommandNavigationProps) {
  return <section class="command-navigation" data-component="command-navigation">
    <button type="button" class="command-navigation__heading" data-action="toggle-command-group" aria-expanded={String(expanded)}><span>{label}</span><LucideIcon icon={ChevronDown} name="chevron-down" /></button>
    {expanded && <div class="command-navigation__items">{commands.map(command => { const [icon, name] = icons[command.icon]; const color = resolveCustomizationColor(command.color); return <button type="button" class="command-navigation__command" style={`--command-color:${color};--command-text-color:${customizationContrastColor(color)}`} data-command-color={color} data-action="run-command" data-command-id={command.id} aria-pressed={String(Boolean(command.running))}><LucideIcon icon={icon} name={name} /><span>{command.running ? `Running ${command.label}` : command.label}</span>{command.running && <i aria-hidden="true"></i>}</button>; })}</div>}
  </section>;
}
