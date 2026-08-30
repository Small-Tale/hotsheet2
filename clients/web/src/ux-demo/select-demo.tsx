import { Bug, ListChecks, Sparkles } from 'lucide';

import { Select } from '../components/select';

const choices = [
  { value: 'task', label: 'Task', icon: ListChecks, iconName: 'list-checks', color: '#14b8a6' },
  { value: 'feature', label: 'Feature', icon: Sparkles, iconName: 'sparkles', color: '#8b5cf6' },
  { value: 'bug', label: 'Bug', icon: Bug, iconName: 'bug', color: '#ef4444' },
] as const;

export function SelectDemo() { return <section class="metadata-control-demo" aria-label="Select demo"><Select name="example-select" label="Ticket type" value="feature" choices={choices} /></section>; }
