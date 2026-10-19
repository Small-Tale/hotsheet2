export function projectSettingsValue<T>(
  values: Readonly<Record<string, T>>,
  projectId: string,
  fallback: NoInfer<T>,
): T {
  return values[projectId] ?? fallback;
}

export function updateProjectSettingsValue<T>(
  values: Readonly<Record<string, T>>,
  projectId: string,
  value: NoInfer<T>,
): Record<string, T> {
  return { ...values, [projectId]: value };
}
