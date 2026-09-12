import type { ChangeEvent } from './api';

interface AcknowledgedChange {
  key: string;
  acknowledgedAt: number;
}

const MAX_ACKNOWLEDGEMENTS = 2_048;
const ACKNOWLEDGEMENT_TTL_MS = 30_000;

const changeKey = (projectId: string, change: Pick<ChangeEvent, 'store' | 'id' | 'kind'>): string => `${projectId}\0${change.store}\0${change.kind}\0${change.id}`;

/**
 * Matches server change-stream events to mutations whose returned ticket has already
 * been projected into the local UI. Exact counted matches keep concurrent external
 * writes visible: only one event is consumed for each acknowledged local write.
 */
export class LocalTicketChangeAcknowledgements {
  private acknowledged: AcknowledgedChange[] = [];

  acknowledge(projectId: string, change: Pick<ChangeEvent, 'store' | 'id' | 'kind'>, now = Date.now()): void {
    this.prune(now);
    this.acknowledged.push({ key: changeKey(projectId, change), acknowledgedAt: now });
    if (this.acknowledged.length > MAX_ACKNOWLEDGEMENTS) {
      this.acknowledged.splice(0, this.acknowledged.length - MAX_ACKNOWLEDGEMENTS);
    }
  }

  unacknowledged(projectId: string, events: readonly ChangeEvent[], now = Date.now()): ChangeEvent[] {
    this.prune(now);
    return events.filter(event => {
      const index = this.acknowledged.findIndex(change => change.key === changeKey(projectId, event));
      if (index < 0) return true;
      this.acknowledged.splice(index, 1);
      return false;
    });
  }

  private prune(now: number): void {
    this.acknowledged = this.acknowledged.filter(change => now - change.acknowledgedAt <= ACKNOWLEDGEMENT_TTL_MS);
  }
}
