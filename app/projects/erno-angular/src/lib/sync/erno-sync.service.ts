import { Inject, Injectable, OnDestroy } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject, Subscription, firstValueFrom } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoDatabaseService } from './erno-database.service';
import { ErnoRealtimeService, SyncPushEvent } from '../realtime/erno-realtime.service';
import { ErnoAppStateService } from '../app-state/erno-app-state.service';
import { ErnoNetworkService } from '../network/erno-network.service';

export type SyncStatus = 'idle' | 'syncing' | 'synced' | 'offline' | 'error';

/**
 * Normalized change applied to the local store.
 *
 * Delta pulls map each server row into this shape (`deleted` is true when
 * `deleted_at` is set). Realtime push events already look like this.
 */
export interface SyncDeltaItem {
  entity: string;
  id: string;
  sync_seq: number;
  deleted: boolean;
  /** Full row from delta pull, or the push snapshot when available. */
  data: unknown;
}

/** Server response from `GET {deltaPath}?since=N` (see erno sync_delta). */
export interface SyncDeltaResponse {
  items: Array<
    Record<string, unknown> & {
      id: string;
      sync_seq: number;
      deleted_at?: string | null;
    }
  >;
  next_since: number;
}

interface EntityRegistration {
  /** Path relative to baseUrl, e.g. `/api/sessions/sync`. */
  deltaPath: string;
  handler: (item: SyncDeltaItem) => Promise<void>;
}

/**
 * Pulls per-entity deltas and applies realtime push events.
 *
 * Apps register each syncable entity with its delta endpoint (mounted via
 * `sync_delta` / `sync_delta_shared` on the API), then call `start()` once.
 */
@Injectable()
export class ErnoSyncService implements OnDestroy {
  private _status = new BehaviorSubject<SyncStatus>('idle');
  readonly status$ = this._status.asObservable();

  private entityHandlers = new Map<string, EntityRegistration>();
  private started = false;
  private pullInFlight: Promise<void> | null = null;
  private subscriptions = new Subscription();

  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
    private db: ErnoDatabaseService,
    private realtime: ErnoRealtimeService,
    private appState: ErnoAppStateService,
    private network: ErnoNetworkService,
  ) {
    // On foreground resume the realtime socket reconnects on its own; pull a
    // delta to catch up on anything missed while the app was backgrounded.
    this.subscriptions.add(
      this.appState.resumed$.subscribe(() => {
        if (this.started) void this.pullDelta();
      }),
    );
    // Same catch-up when the network returns; socket resume is handled by
    // ErnoRealtimeService.
    this.subscriptions.add(
      this.network.online$.subscribe(() => {
        if (this.started) void this.pullDelta();
      }),
    );
    this.subscriptions.add(
      this.network.offline$.subscribe(() => {
        if (this.started) this._status.next('offline');
      }),
    );
  }

  /**
   * Register a handler for one syncable entity.
   *
   * @param entity Entity type string (must match `Syncable::entity_type()` / table name).
   * @param deltaPath Absolute path under the API host, e.g. `/api/sessions/sync`.
   * @param handler Applies one change to the app's local store.
   */
  register(
    entity: string,
    deltaPath: string,
    handler: (item: SyncDeltaItem) => Promise<void>,
  ): void {
    this.entityHandlers.set(entity, { deltaPath, handler });
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
    if (!this.network.connected) {
      this._status.next('offline');
      return;
    }
    this._status.next('syncing');
    try {
      for (const [entity, reg] of this.entityHandlers) {
        const since = await this.db.getLastSyncSeq(entity);
        const response = await firstValueFrom(
          this.http.get<SyncDeltaResponse>(`${this.config.baseUrl}${reg.deltaPath}`, {
            params: { since },
          }),
        );

        for (const row of response.items) {
          await reg.handler({
            entity,
            id: row.id,
            sync_seq: row.sync_seq,
            deleted: row.deleted_at != null && row.deleted_at !== undefined,
            data: row,
          });
        }
        await this.db.setLastSyncSeq(entity, response.next_since);
      }
      this._status.next('synced');
    } catch {
      this._status.next(this.network.connected ? 'error' : 'offline');
    }
  }

  private async applyPush(event: SyncPushEvent): Promise<void> {
    const reg = this.entityHandlers.get(event.entity);
    if (!reg) return;
    await reg.handler({
      entity: event.entity,
      id: event.id,
      sync_seq: event.sync_seq,
      deleted: event.deleted,
      data: event.data,
    });
    await this.db.setLastSyncSeq(event.entity, event.sync_seq);
  }
}
