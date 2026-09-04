export type TicketPatch = Record<string, unknown>;
export interface TicketSnapshot { slug: string; [field: string]: unknown }
interface Change { slug: string; before: TicketPatch; after: TicketPatch }
type Transaction = { kind:'fields';changes:Change[] } | { kind:'external';undo:()=>Promise<boolean>;redo:()=>Promise<boolean> };

/** Project-scoped, field-aware history. Remote changes win over stale undo entries. */
export class TicketHistory {
  private undoStack: Transaction[] = [];
  private redoStack: Transaction[] = [];
  constructor(private readonly read: (slug: string) => TicketSnapshot | undefined, private readonly apply: (slug: string, patch: TicketPatch) => Promise<boolean>) {}
  async execute(slug: string, patch: TicketPatch): Promise<boolean> {
    return this.executeMany([{ slug, patch }]);
  }
  async executeMany(operations: Array<{slug:string;patch:TicketPatch}>): Promise<boolean> {
    const changes = operations.flatMap(({slug,patch}) => { const current=this.read(slug); return current?[{slug,before:Object.fromEntries(Object.keys(patch).map(key=>[key,current[key]])),after:patch}]:[]; });
    if (changes.length !== operations.length) return false;
    for (const change of changes) if (!await this.apply(change.slug,change.after)) return false;
    this.undoStack.push({kind:'fields',changes}); this.redoStack=[]; return true;
  }
  recordExternal(undo:()=>Promise<boolean>,redo:()=>Promise<boolean>):void { this.undoStack.push({kind:'external',undo,redo});this.redoStack=[]; }
  async undo(): Promise<boolean> { return this.move(this.undoStack, this.redoStack, true); }
  async redo(): Promise<boolean> { return this.move(this.redoStack, this.undoStack, false); }
  private async move(source: Transaction[], destination: Transaction[], undo: boolean): Promise<boolean> {
    const transaction = source.at(-1); if (!transaction) return false;
    if(transaction.kind==='external'){if(!await (undo?transaction.undo():transaction.redo()))return false;source.pop();destination.push(transaction);return true}
    for (const change of undo?[...transaction.changes].reverse():transaction.changes) {
      const current = this.read(change.slug); if (!current) return false;
      const patch = matchingFields(current, undo ? change.after : change.before, undo ? change.before : change.after);
      if (Object.keys(patch).length && !await this.apply(change.slug, patch)) return false;
    }
    source.pop(); destination.push(transaction); return true;
  }
}

function matchingFields(current: TicketSnapshot, expected: TicketPatch, replacement: TicketPatch): TicketPatch {
  return Object.fromEntries(Object.keys(expected).filter(key => JSON.stringify(current[key]) === JSON.stringify(expected[key])).map(key => [key, replacement[key]]));
}

export interface ClipboardTicket { id: string; slug: string; connection_id: string; native_id: string; title: string; details: string; category: string; priority?: string; status?: string; up_next: boolean; tags: string[]; notes: Array<{kind:string;text:string;summary?:string}>; attachments:Array<{id:string;filename:string}> }
export function deduplicateTitle(title: string, existing: readonly string[]): string {
  const occupied = new Set(existing.map(value => value.toLocaleLowerCase()));
  if (!occupied.has(title.toLocaleLowerCase())) return title;
  if (!occupied.has(`${title} (Copy)`.toLocaleLowerCase())) return `${title} (Copy)`;
  let suffix = 2; while (occupied.has(`${title} (Copy ${suffix})`.toLocaleLowerCase())) suffix += 1;
  return `${title} (Copy ${suffix})`;
}
