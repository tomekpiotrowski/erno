import { Inject, Injectable, OnDestroy } from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject, Observable, Subscription, firstValueFrom } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import {
  ErnoRealtimeService,
  ShareSubscription,
  SyncPushEvent,
} from '../realtime/erno-realtime.service';
import { SHARE_TOKEN_HEADER } from './erno-share.service';

interface SyncDeltaResponse {
  items: Array<Record<string, unknown> & { id: string; deleted_at?: string | null }>;
  next_since: number;
}

interface OpenShare {
  token: string;
  subscription: ShareSubscription;
  /** Per-share delta cursor — independent of the durable per-user lastSyncSeq. */
  sinces: Map<string, number>;
}

/**
 * Online-only view over shared data.
 *
 * Shared rows are held in memory and never written to the durable per-user
 * IndexedDB store: the owner's offline dataset stays pristine, and revocation
 * cleanup is simply dropping the view. Real-time updates arrive over the
 * WebSocket connection once the share is subscribed.
 *
 * Apps register each shared entity's delta endpoint once:
 * ```ts
 * sharedView.registerEntity('posts', '/api/posts/sync');
 * sharedView.registerEntity('comments', '/api/comments/sync');
 * await sharedView.open(shareService.tokenFromLocation()!);
 * sharedView.items$('comments').subscribe(...);
 * ```
 */
@Injectable()
export class ErnoSharedViewService implements OnDestroy {
  private entityEndpoints = new Map<string, string>();
  private stores = new Map<string, BehaviorSubject<Record<string, unknown>[]>>();
  private itemsByEntity = new Map<string, Map<string, Record<string, unknown>>>();
  private openShares = new Map<string, OpenShare>();
  private pushSubscription: Subscription | null = null;
  private revokeSubscription: Subscription | null = null;

  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private http: HttpClient,
    private realtime: ErnoRealtimeService,
  ) {}

  /** Register the share-aware delta endpoint for an entity type. */
  registerEntity(entity: string, deltaPath: string): void {
    this.entityEndpoints.set(entity, deltaPath);
  }

  /** Reactive view of the shared rows currently held for an entity type. */
  items$(entity: string): Observable<Record<string, unknown>[]> {
    return this.storeFor(entity).asObservable();
  }

  /**
   * Open a shared view from a raw link token: attaches the share to the
   * WebSocket connection (live push) and pulls the share-scoped delta for
   * every registered entity into the in-memory store.
   */
  async open(token: string): Promise<ShareSubscription> {
    const subscription = await this.realtime.subscribeShare(token);
    this.openShares.set(subscription.share_id, {
      token,
      subscription,
      sinces: new Map(),
    });
    this.ensureStreams();
    await this.pull(subscription.share_id);
    return subscription;
  }

  /** Catch up via delta for one open share (e.g. after a reconnect). */
  async pull(shareId: string): Promise<void> {
    const open = this.openShares.get(shareId);
    if (!open) return;

    for (const [entity, path] of this.entityEndpoints) {
      const since = open.sinces.get(entity) ?? 0;
      const response = await firstValueFrom(
        this.http.get<SyncDeltaResponse>(`${this.config.baseUrl}${path}`, {
          params: { since },
          headers: { [SHARE_TOKEN_HEADER]: open.token },
        }),
      );
      for (const item of response.items) {
        this.applyItem(entity, item);
      }
      open.sinces.set(entity, response.next_since);
    }
  }

  /** Close one shared view: detach from the socket and drop its data. */
  async close(shareId: string): Promise<void> {
    if (!this.openShares.delete(shareId)) return;
    try {
      await this.realtime.unsubscribeShare(shareId);
    } catch {
      // Connection may already be gone; dropping local state is what matters.
    }
    if (this.openShares.size === 0) {
      this.clearAll();
    }
  }

  ngOnDestroy(): void {
    this.pushSubscription?.unsubscribe();
    this.revokeSubscription?.unsubscribe();
    this.clearAll();
  }

  private ensureStreams(): void {
    this.pushSubscription ??= this.realtime.events$.subscribe(event =>
      this.applyPush(event),
    );
    // A revoked share closes its view and drops its rows immediately.
    this.revokeSubscription ??= this.realtime.shareEvents$.subscribe(event => {
      if (event.type === 'share-revoked' && this.openShares.has(event.share_id)) {
        this.openShares.delete(event.share_id);
        if (this.openShares.size === 0) {
          this.clearAll();
        }
      }
    });
  }

  private applyPush(event: SyncPushEvent): void {
    if (this.openShares.size === 0) return;
    if (!this.entityEndpoints.has(event.entity)) return;
    const data = event.data as Record<string, unknown> | null;
    this.applyItem(event.entity, {
      id: event.id,
      ...(data ?? {}),
      deleted_at: event.deleted
        ? new Date().toISOString()
        : ((data?.['deleted_at'] as string | null | undefined) ?? null),
    });
  }

  private applyItem(
    entity: string,
    item: Record<string, unknown> & { id: string; deleted_at?: string | null },
  ): void {
    const items = this.itemsByEntity.get(entity) ?? new Map();
    if (item.deleted_at != null) {
      items.delete(item.id);
    } else {
      items.set(item.id, item);
    }
    this.itemsByEntity.set(entity, items);
    this.storeFor(entity).next([...items.values()]);
  }

  private storeFor(entity: string): BehaviorSubject<Record<string, unknown>[]> {
    let store = this.stores.get(entity);
    if (!store) {
      store = new BehaviorSubject<Record<string, unknown>[]>([]);
      this.stores.set(entity, store);
    }
    return store;
  }

  private clearAll(): void {
    this.itemsByEntity.clear();
    for (const store of this.stores.values()) {
      store.next([]);
    }
  }
}
