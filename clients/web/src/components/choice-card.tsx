import './choice-card.css';

import type { SafeHtml } from 'kerfjs';

/** Checked-state emphasis: `brand` for ordinary choices, `danger` for destructive ones. */
export type ChoiceCardTone = 'brand' | 'danger';

export interface ChoiceCardProps {
  /** The native input kind; radio cards share a `name` group, checkbox cards toggle alone. */
  type: 'radio' | 'checkbox';
  /** Form name the owning feature's delegated handlers and tests read the input by. */
  name: string;
  /** Radio value (or checkbox value when the form submits it). */
  value?: string;
  checked?: boolean;
  disabled?: boolean;
  /** Short option title. */
  title: string | SafeHtml;
  /** Quiet supporting description below the title. */
  description?: string | SafeHtml;
  /** Checked emphasis. Default `brand`. */
  tone?: ChoiceCardTone;
}

/**
 * A selectable option card: a label framing a native radio or checkbox with a title, a quiet
 * description, and checked (brand or danger) and disabled states (HS2-V7PCS6). The input stays
 * native so form wiring and keyboard behavior are the browser's, and its owning feature reads it
 * by `name`.
 */
export function ChoiceCard({
  type,
  name,
  value,
  checked = false,
  disabled = false,
  title,
  description,
  tone = 'brand',
}: ChoiceCardProps) {
  return (
    <label class="choice-card" data-component="choice-card" data-tone={tone}>
      <input type={type} name={name} value={value} checked={checked} disabled={disabled} />
      <span class="choice-card__copy">
        <strong>{title}</strong>
        {description === undefined ? <></> : <small>{description}</small>}
      </span>
    </label>
  );
}
