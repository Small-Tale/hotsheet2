import '@kerfjs/ui/chip.css';
import './tag-chip.css';

import { Chip } from '@kerfjs/ui/chip';

export type TagChipVariant = 'brand' | 'neutral' | 'success' | 'warning' | 'danger';
export type TagChipAppearance = 'accent' | 'filled' | 'outlined' | 'filled-outlined';
export type TagChipSize = 'small' | 'medium' | 'large';

/** The delegated action Kerf's remove button carries; the owning feature handles the click. */
export const TAG_CHIP_REMOVE_ACTION = 'remove-tag-chip';

export interface TagChipProps {
  id: string;
  label: string;
  variant?: TagChipVariant;
  appearance?: TagChipAppearance;
  size?: TagChipSize;
  removable?: boolean;
  pill?: boolean;
  disabled?: boolean;
}

export interface NormalizedTagChipProps extends Required<Omit<TagChipProps, 'label'>> {
  label: string;
}

export function normalizeTagChipProps(props: TagChipProps): NormalizedTagChipProps {
  return {
    id: props.id,
    label: props.label.trim() || 'Untitled tag',
    variant: props.variant ?? 'neutral',
    appearance: props.appearance ?? 'filled',
    size: props.size ?? 'small',
    removable: props.removable ?? false,
    pill: props.pill ?? false,
    disabled: props.disabled ?? false,
  };
}

/** Kerf's semantic tones name the brand palette `info` (5.0.0-beta.76). */
const CHIP_TONE = {
  brand: 'info',
  neutral: 'neutral',
  success: 'success',
  warning: 'warning',
  danger: 'danger',
} as const;

const CHIP_APPEARANCE = {
  accent: 'solid',
  filled: 'quiet',
  outlined: 'outline',
  'filled-outlined': 'outline',
} as const;

/**
 * A domain tag on Kerf's `Chip` (HS2-HJEHRW): the app wrapper carries the stable tag identity and
 * the parent owns the mutation that the chip's delegated remove action requests.
 */
export function TagChip(raw: TagChipProps) {
  const props = normalizeTagChipProps(raw);
  const presentation = {
    tone: CHIP_TONE[props.variant],
    appearance: CHIP_APPEARANCE[props.appearance],
    shape: props.pill ? 'pill' : 'rounded',
    size: props.size === 'small' ? 'compact' : 'default',
    disabled: props.disabled,
    itemId: props.id,
  } as const;
  return (
    <span class="tag-chip" data-component="tag-chip" data-tag-id={props.id} data-disabled={String(props.disabled)}>
      {props.removable ? (
        <Chip {...presentation} removeAction={TAG_CHIP_REMOVE_ACTION} removeLabel={`Remove ${props.label}`}>
          {props.label}
        </Chip>
      ) : (
        <Chip {...presentation}>{props.label}</Chip>
      )}
    </span>
  );
}
