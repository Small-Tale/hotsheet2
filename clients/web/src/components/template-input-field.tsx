import { Checkbox } from '@kerfjs/ui/checkbox';
import { Select } from '@kerfjs/ui/select';
import { TextField } from '@kerfjs/ui/text-field';

import type { TemplateInput } from '../orchestration-templates';

const PRIORITY_CHOICES = ['highest', 'high', 'default', 'low', 'lowest'].map((value) => ({
  value,
  label: value.charAt(0).toUpperCase() + value.slice(1),
}));

export interface TemplateInputFieldProps {
  input: TemplateInput;
  value: unknown;
  /** The declared projects a project input offers. */
  projects: ReadonlyArray<{ target_id: string; label: string }>;
  /** The field's name, which the dialog's delegate reads the input's key from. */
  name: string;
  /** The field's label; the input's own label when absent. */
  label?: string;
  /** A first choice that leaves a choice, priority or project input unset (the template's default). */
  emptyChoice?: string;
  /** A text or number field's placeholder, such as the template's default. */
  placeholder?: string;
  rootAttributes?: Record<string, string>;
}

/**
 * One typed template input: a checkbox for a boolean, the declared projects for a project input, the priorities
 * for a priority, the template's choices for a choice, otherwise a text or number field. Use Template… and the
 * Triggers… setup's fixed values share it (HS2-40GPZK), so a fixed value cannot be mistyped.
 */
export function TemplateInputField({
  input,
  value,
  projects,
  name,
  label = input.label,
  emptyChoice,
  placeholder,
  rootAttributes = {},
}: TemplateInputFieldProps) {
  switch (input.kind) {
    case 'boolean':
      return (
        <Checkbox
          name={name}
          label={label}
          checked={value === true}
          hint={input.hint}
          rootAttributes={rootAttributes}
        />
      );
    case 'choice':
    case 'priority':
    case 'target': {
      const choices =
        input.kind === 'choice'
          ? input.choices.map((choice) => ({ value: choice, label: choice }))
          : input.kind === 'priority'
            ? PRIORITY_CHOICES
            : projects.map((project) => ({ value: project.target_id, label: project.label }));
      return (
        <Select
          name={name}
          label={label}
          value={typeof value === 'string' ? value : ''}
          choices={emptyChoice === undefined ? choices : [{ value: '', label: emptyChoice }, ...choices]}
          placeholderText={input.kind === 'target' ? 'Choose a project' : 'Choose'}
          hint={input.hint}
          rootAttributes={rootAttributes}
        />
      );
    }
    case 'text':
    case 'number':
    case 'tool':
      return (
        <TextField
          name={name}
          label={label}
          value={typeof value === 'string' || typeof value === 'number' ? String(value) : ''}
          type={input.kind === 'number' ? 'number' : 'text'}
          hint={input.hint}
          required={input.required && emptyChoice === undefined}
          {...(placeholder === undefined ? {} : { placeholder })}
          rootAttributes={rootAttributes}
        />
      );
    default:
      return input.kind satisfies never;
  }
}
