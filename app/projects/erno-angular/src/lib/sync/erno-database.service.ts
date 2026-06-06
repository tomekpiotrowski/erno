import { Injectable } from '@angular/core';
import Dexie, { Table } from 'dexie';

export interface SyncMeta {
  entity: string;
  lastSyncSeq: number;
}

export interface PendingMutation {
  id?: number;
  entity: string;
  entityId: string;
  payload: unknown;
  operation: 'upsert' | 'delete';
  createdAt: number;
}

@Injectable()
export class ErnoDatabaseService extends Dexie {
  syncMeta!: Table<SyncMeta, string>;
  pendingMutations!: Table<PendingMutation, number>;

  constructor() {
    super('erno');
    this.version(1).stores({
      syncMeta: 'entity',
      pendingMutations: '++id, entity, entityId, createdAt',
    });
  }

  async getLastSyncSeq(entity: string): Promise<number> {
    return (await this.syncMeta.get(entity))?.lastSyncSeq ?? 0;
  }

  async setLastSyncSeq(entity: string, seq: number): Promise<void> {
    await this.syncMeta.put({ entity, lastSyncSeq: seq });
  }

  /**
   * Wipe all locally cached sync state — sync cursors and queued offline
   * mutations. Used on account deletion so nothing leaks to the next user on a
   * shared device. Apps with their own IndexedDB stores should clear those too.
   */
  async clear(): Promise<void> {
    await Promise.all(this.tables.map((table) => table.clear()));
  }
}
