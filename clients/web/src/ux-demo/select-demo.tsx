import { CatalogExample } from '@kerfjs/ui/catalog';
import { foregroundColorVar } from '@kerfjs/ui/css-values';
import { Select } from '@kerfjs/ui/select';
import { Bug, ListChecks, Sparkles } from 'lucide';

import type { DemoModule } from './demo-module';

const choices = [
  {
    value: 'task',
    label: 'Task',
    icon: ListChecks,
    iconName: 'list-checks',
    color: foregroundColorVar('--hs-category-task'),
  },
  {
    value: 'feature',
    label: 'Feature',
    icon: Sparkles,
    iconName: 'sparkles',
    color: foregroundColorVar('--hs-category-feature'),
  },
  { value: 'bug', label: 'Bug', icon: Bug, iconName: 'bug', color: foregroundColorVar('--hs-category-bug') },
] as const;

export function SelectDemo() {
  return (
    <CatalogExample label="Interactive" viewport={{ width: 'control' }}>
      <Select name="example-select" label="Ticket type" value="feature" choices={choices} />
    </CatalogExample>
  );
}

/** The catalog entries this file renders (HS2-HMVBZW). */
export const demoModule: DemoModule = {
  entries: {
    select: { catalogExamples: true, Demo: SelectDemo },
  },
};
