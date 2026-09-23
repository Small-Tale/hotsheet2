import { colorVar } from '@kerfjs/ui/css-values';
import { Select } from '@kerfjs/ui/select';
import { Bug, ListChecks, Sparkles } from 'lucide';

const choices = [
  { value: 'task', label: 'Task', icon: ListChecks, iconName: 'list-checks', color: colorVar('--hs-category-task') },
  {
    value: 'feature',
    label: 'Feature',
    icon: Sparkles,
    iconName: 'sparkles',
    color: colorVar('--hs-category-feature'),
  },
  { value: 'bug', label: 'Bug', icon: Bug, iconName: 'bug', color: colorVar('--hs-category-bug') },
] as const;

export function SelectDemo() {
  return (
    <section class="metadata-control-demo" aria-label="Select demo">
      <Select name="example-select" label="Ticket type" value="feature" choices={choices} />
    </section>
  );
}
