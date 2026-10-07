import { type Capabilities, type ProviderDescriptor } from './api';

/** One ticket source a project can address, projected from its provider descriptor (HS2-NZMJBJ). */
export interface TicketSourceChoice {
  connectionId: string;
  name: string;
  /** Provider kind (`git`, `github`, …) and its locator, for the Ticket sources panel (HS2-3SCH1K). */
  provider: string;
  locator: string;
  capabilities: Capabilities;
  default: boolean;
  color?: string;
}

/**
 * A project's ticket sources: the default source's name and capabilities (what most surfaces
 * consult) plus every source the project can address, so the new-ticket composer can offer a
 * target choice when more than one can create tickets.
 */
export interface ProjectTicketSources {
  name: string;
  capabilities: Capabilities;
  connectionId?: string;
  sources: readonly TicketSourceChoice[];
}

/** Project the provider descriptors into the default source plus every addressable source. */
export function projectTicketSources(descriptors: readonly ProviderDescriptor[]): ProjectTicketSources | undefined {
  const selected = descriptors.find((item) => item.default) ?? descriptors.at(0);
  if (!selected) return undefined;
  return {
    name: selected.display_name,
    capabilities: selected.capabilities,
    connectionId: selected.connection_id,
    sources: descriptors.map((item) => ({
      connectionId: item.connection_id,
      name: item.display_name,
      provider: item.provider,
      locator: item.locator,
      capabilities: item.capabilities,
      default: item === selected,
      color: item.color,
    })),
  };
}

/** Sources that accept new tickets, in descriptor order. */
export function writableTicketSources(project: ProjectTicketSources | undefined): TicketSourceChoice[] {
  return (project?.sources ?? []).filter((source) => source.capabilities.create);
}

/**
 * The source a new ticket targets: the most recently used one for this project while it is still
 * writable, otherwise the default source when it is writable, otherwise the first writable source.
 * With no writable source it is the default source, so callers still see its `create: false`.
 */
export function resolveNewTicketSource(
  project: ProjectTicketSources | undefined,
  preferred?: string,
): TicketSourceChoice | undefined {
  const writable = writableTicketSources(project);
  return (
    (preferred ? writable.find((source) => source.connectionId === preferred) : undefined) ??
    writable.find((source) => source.default) ??
    writable.at(0) ??
    project?.sources.find((source) => source.default)
  );
}

/** Route explicitly whenever the selected source differs from the checkout default. */
export function newTicketCreationSourceId(
  project: ProjectTicketSources | undefined,
  target: TicketSourceChoice | undefined,
): string | undefined {
  return target && (writableTicketSources(project).length > 1 || !target.default) ? target.connectionId : undefined;
}

/** Remember the source a ticket was just created in, per project, in memory only. */
export function rememberNewTicketSource(
  remembered: Readonly<Record<string, string>>,
  projectId: string,
  connectionId: string,
): Record<string, string> {
  return { ...remembered, [projectId]: connectionId };
}
