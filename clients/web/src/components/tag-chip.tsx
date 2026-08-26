export type TagChipVariant = 'brand' | 'neutral' | 'success' | 'warning' | 'danger';
export type TagChipAppearance = 'accent' | 'filled' | 'outlined' | 'filled-outlined';
export type TagChipSize = 'small' | 'medium' | 'large';

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
    appearance: props.appearance ?? 'filled-outlined',
    size: props.size ?? 'small',
    removable: props.removable ?? false,
    pill: props.pill ?? true,
    disabled: props.disabled ?? false,
  };
}

/** A domain tag whose parent owns mutations triggered by bubbling `wa-remove`. */
export function TagChip(raw: TagChipProps) {
  const props = normalizeTagChipProps(raw);
  return (
    <wa-tag
      data-component="tag-chip"
      data-tag-id={props.id}
      data-disabled={props.disabled ? 'true' : 'false'}
      aria-disabled={props.disabled ? 'true' : 'false'}
      variant={props.variant}
      appearance={props.appearance}
      size={props.size}
      pill={props.pill}
      with-remove={props.removable && !props.disabled}
    >
      {props.label}
    </wa-tag>
  );
}
