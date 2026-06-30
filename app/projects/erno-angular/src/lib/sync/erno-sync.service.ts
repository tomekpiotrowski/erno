import { Inject, Injectable, OnDestroy } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject, Subscription } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoDatabaseService } from './erno-database.service';
import { ErnoRealtimeService, SyncPushEvent } from '../realtime/erno-realtime.service';
import { ErnoAppStateService } from '../app-state/erno-app-state.service';

export type SyncStatus = 'idle' | 'syncing' | 'synced' | 'offline' | 'error';

export interface SyncDeltaItem {
  entity: string;
  id: string;
  sync_seq: number;
  deleted: boolean;
  data: unknown;
}

@Injectable()
export class ErnoSyncService implements OnDestroy {
  private _status = new BehaviorSubject<SyncStatus>('idle');
  readonly status$ = this._status.asObservable();

  private entityHandlers = new Map<string, (item: SyncDeltaItem) => Promise<void>>();
  private started = false;
  private pullInFlight: Promise<void> | null = null;
  private subscriptions = new Subscription();

  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
    private db: ErnoDatabaseService,
    private realtime: ErnoRealtimeService,
    private appState: ErnoAppStateService,
  ) {
    // On foreground resume the realtime socket reconnects on its own; pull a
    // delta to catch up on anything missed while the app was backgrounded.
    this.subscriptions.add(
      this.appState.resumed$.subscribe(() => {
        if (this.started) void this.pullDelta();
      }),
    );
  }

  register<T>(entity: string, handler: (item: SyncDeltaItem) => Promise<void>): void {
    this.entityHandlers.set(entity, handler);
  }

  async start(): Promise<void> {
    if (this.started) return;
    this.started = true;
    this.subscriptions.add(this.realtime.events$.subscribe(event => this.applyPush(event)));
    this.realtime.connect();
    await this.pullDelta();
  }

  ngOnDestroy(): void {
    this.subscriptions.unsubscribe();
  }

  pullDelta(): Promise<void> {
    if (this.pullInFlight) return this.pullInFlight;
    this.pullInFlight = this.doPullDelta().finally(() => {
      this.pullInFlight = null;
    });
    return this.pullInFlight;
  }

  private async doPullDelta(): Promise<void> {
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
