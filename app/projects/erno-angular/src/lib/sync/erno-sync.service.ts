import { Inject, Injectable } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoDatabaseService } from './erno-database.service';
import { ErnoRealtimeService, SyncPushEvent } from '../realtime/erno-realtime.service';

export type SyncStatus = 'idle' | 'syncing' | 'synced' | 'offline' | 'error';

export interface SyncDeltaItem {
  entity: string;
  id: string;
  sync_seq: number;
  deleted: boolean;
  data: unknown;
}

@Injectable()
export class ErnoSyncService {
  private _status = new BehaviorSubject<SyncStatus>('idle');
  readonly status$ = this._status.asObservable();

  private entityHandlers = new Map<string, (item: SyncDeltaItem) => Promise<void>>();

  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
    private db: ErnoDatabaseService,
    private realtime: ErnoRealtimeService,
  ) {}

  register<T>(entity: string, handler: (item: SyncDeltaItem) => Promise<void>): void {
    this.entityHandlers.set(entity, handler);
  }

  async start(): Promise<void> {
    this.realtime.events$.subscribe(event => this.applyPush(event));
    this.realtime.connect();
    await this.pullDelta();
  }

  async pullDelta(): Promise<void> {
    this._status.next('syncing');
    try {
      for (const [entity, handler] of this.entityHandlers) {
        const since = await this.db.getLastSyncSeq(entity);
        const items = await this.http
          .get<SyncDeltaItem[]>(`${this.config.baseUrl}/api/sync/delta`, { params: { entity, since } })
          .toPromise();

        if (!items?.length) continue;

        for (const item of items) {
          await handler(item);
        }
        const maxSeq = Math.max(...items.map(i => i.sync_seq));
        await this.db.setLastSyncSeq(entity, maxSeq);
      }
      this._status.next('synced');
    } catch {
      this._status.next('error');
    }
  }

  private async applyPush(event: SyncPushEvent): Promise<void> {
    const handler = this.entityHandlers.get(event.entity);
    if (!handler) return;
    await handler({ ...event, data: null });
    await this.db.setLastSyncSeq(event.entity, event.sync_seq);
  }
}
