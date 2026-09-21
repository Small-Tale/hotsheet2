export interface CopiedTicketPlacement {
  status?: string;
  up_next: boolean;
}

export function copiedTicketPlacement(source: CopiedTicketPlacement) {
  return {
    status: 'not_started' as const,
    up_next: source.up_next,
  };
}
