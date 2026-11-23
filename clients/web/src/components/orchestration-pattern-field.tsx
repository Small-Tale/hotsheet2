import { Chip } from '@kerfjs/ui/chip';
import { Combobox } from '@kerfjs/ui/combobox';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';

import { ORCHESTRATION_SETTINGS_ACTIONS as ACTIONS } from '../interaction-attrs/orchestration-settings';

/** The combobox id prefix the app's combobox wiring routes to the Limits form (HS2-1X1TXC). */
export const ORCHESTRATION_PATTERN_FIELD_PREFIX = 'orchestration-limits-';

export interface OrchestrationPatternFieldProps {
  /** The Limits draft field it edits: `limits.resources` or `limits.models.<tool>`. */
  field: string;
  label: string;
  /** The chosen names and patterns, in order. */
  values: readonly string[];
  /** Known names and patterns to choose from; typing another one uses it as typed. */
  suggestions: readonly string[];
  hint?: string;
  placeholder?: string;
  disabled?: boolean;
}

/**
 * HS2-1X1TXC, HS2-NPAZQW: a list of names a Limits field holds (a tool's models, the required
 * resources), each a removable chip, chosen from a Kerf Combobox of known names and `*` patterns
 * (`opus-*`, `vm/*`) or typed. One control for both fields, so they behave alike. The combobox's
 * commit and a chip's remove reach the Limits draft through the orchestration settings wiring.
 */
export function OrchestrationPatternField({
  field,
  label,
  values,
  suggestions,
  hint,
  placeholder,
  disabled = false,
}: OrchestrationPatternFieldProps) {
  return (
    <div data-component="orchestration-pattern-field" data-limits-field={field}>
      <List gap="xs">
        <Combobox
          id={`${ORCHESTRATION_PATTERN_FIELD_PREFIX}${field.replaceAll('.', '-')}`}
          label={label}
          suggestions={suggestions.filter((suggestion) => !values.includes(suggestion))}
          placeholder={placeholder}
          hint={hint}
          createLabel="Use “{value}”"
          disabled={disabled}
          rootAttributes={{ 'data-limits-field': field }}
        />
        {values.length ? (
          <Row wrap gap="xs">
            {values.map((value) => (
              <Chip
                itemId={value}
                tone="neutral"
                appearance="quiet"
                size="compact"
                truncate
                disabled={disabled}
                removeAction={ACTIONS.removeLimitsValue.value}
                removeLabel={`Remove ${value}`}
              >
                {value}
              </Chip>
            ))}
          </Row>
        ) : null}
      </List>
    </div>
  );
}
