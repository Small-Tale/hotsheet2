import type { CustomView } from './api';

function baseViewId(name: string): string {
  const normalized = name.normalize('NFKD').replace(/[\u0300-\u036f]/g, '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 54);
  return normalized || 'view';
}

export function uniqueCustomViewId(name: string, existing: readonly CustomView[]): string {
  const base = baseViewId(name), ids = new Set(existing.map(view => view.id.toLowerCase()));
  if (!ids.has(base)) return base;
  for (let suffix = 2; suffix < 10_000; suffix += 1) {
    const id = `${base}-${suffix}`;
    if (!ids.has(id)) return id;
  }
  return `${base}-${Date.now().toString(36)}`;
}

export function customViewNameAvailable(name: string, existing: readonly CustomView[]): boolean {
  const normalized = name.trim().toLocaleLowerCase();
  return Boolean(normalized) && !existing.some(view => view.name.trim().toLocaleLowerCase() === normalized);
}
