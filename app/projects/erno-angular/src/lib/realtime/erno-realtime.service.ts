import { Inject, Injectable, OnDestroy } from '@angular/core';
import { BehaviorSubject, Observable, Subject, Subscription, firstValueFrom } from 'rxjs';
import { distinctUntilChanged } from 'rxjs/operators';
import { webSocket, WebSocketSubject } from 'rxjs/webSocket';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoAuthService, jwtAccessTokenExpired } from '../auth/erno-auth.service';
import { ErnoAppState, ErnoAppStateService } from '../app-state/erno-app-state.service';
import { ErnoNetworkService } from '../network/erno-network.service';

/** A sync change event, mapped from the backend's application broadcasts. */
export interface SyncPushEvent {
  entity: string;
  id: string;
  sync_seq: number;
  deleted: boolean;
  /** Full row snapshot at change time, when provided by the backend. */
  data: unknown;
}

/** Share lifecycle notifications pushed by the backend. */
export interface ShareEvent {
  type: 'share-granted' | 'share-revoked';
  share_id: string;
  entity_type?: string;
  entity_id?: string;
}

/** Result of a successful subscribe-share request. */
export interface ShareSubscription {
  share_id: string;
  entity_type: string;
  entity_id: string;
}

/**
 * Wire format of backend WebSocket messages (serde internally tagged).
 *
 * Exported because `createSocket` is a protected test seam: a subclass cannot
 * type its override without naming this.
 */
export interface ErnoWsMessage {
  type: 'request' | 'response' | 'broadcast' | 'error';
  id?: string;
  request?: Record<string, unknown>;
  response?: Record<string, unknown> & { type: string };
  broadcast?: Record<string, unknown> & { type: string };
  message?: string;
}

const RECONNECT_DELAY_MS = 3000;

@Injectable()
export class ErnoRealtimeService implements OnDestroy {
  private socket$: WebSocketSubject<ErnoWsMessage> | null = null;
  private connectedSubject$ = new BehaviorSubject<boolean>(false);
  private socketSub: Subscription | null = null;
  private syncEvents$ = new Subject<SyncPushEvent>();
  private shareEventsSubject$ = new Subject<ShareEvent>();
  private pendingRequests = new Map<
    string,
    { resolve: (response: Record<string, unknown> & { type: string }) => void }
  >();
  private requestCounter = 0;

  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  /** Bumped on every `openSocket` so an in-flight refresh cannot bind a stale socket. */
  private openGeneration = 0;
  /** Whether the consumer wants a connection (set by connect/disconnect). */
  private shouldBeConnected = false;
  /** Whether the connection is paused because the app is backgrounded. */
  private backgroundSuspended = false;
  /** Whether the connection is paused because the device is offline. */
  private offlineSuspended = false;
  private lifecycleSubs = new Subscription();

  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private auth: ErnoAuthService,
    private appState: ErnoAppStateService,
    private network: ErnoNetworkService,
  ) {
    this.backgroundSuspended = this.appState.state === 'background';
    this.offlineSuspended = !this.network.connected;
    this.lifecycleSubs.add(this.appState.state$.subscribe(state => this.onAppStateChange(state)));
    this.lifecycleSubs.add(this.network.connected$.subscribe(connected => this.onNetworkChange(connected)));
  }

  /** True when backgrounded or offline — socket must stay closed. */
  private get suspended(): boolean {
    return this.backgroundSuspended || this.offlineSuspended;
  }

  /**
   * Whether the socket is currently open.
   *
   * Driven by the socket's own open/close lifecycle, not by `subscribe()` —
   * `webSocket()` connects lazily and the open lands asynchronously, so a
   * failing reconnect must not report itself as connected. Consumers use the
   * false -> true edge to catch up on anything missed while the socket was
   * down; a silent drop fires no app-state or network event.
   */
  get connected$(): Observable<boolean> {
    return this.connectedSubject$.pipe(distinctUntilChanged());
  }

  /** Sync change events for entities this connection may read. */
  get events$(): Observable<SyncPushEvent> {
    return this.syncEvents$.asObservable();
  }

  /** share-granted / share-revoked notifications for this connection. */
  get shareEvents$(): Observable<ShareEvent> {
    return this.shareEventsSubject$.asObservable();
  }

  /**
   * Connect with the JWT when logged in, anonymously otherwise.
   *
   * Anonymous connections receive nothing until a share is attached via
   * `subscribeShare` — share link tokens are never put in the URL.
   */
  connect(): void {
    this.shouldBeConnected = true;
    this.backgroundSuspended = this.appState.state === 'background';
    this.offlineSuspended = !this.network.connected;
    if (this.suspended) {
      return;
    }
    this.openSocket();
  }

  disconnect(): void {
    this.shouldBeConnected = false;
    this.clearReconnectTimer();
    this.teardownSocket();
  }

  /**
   * Attach a share link token to this connection. The token travels in the
   * message body — post-connect, never in the upgrade URL — and on success
   * the connection starts receiving push events covered by the share.
   */
  async subscribeShare(token: string): Promise<ShareSubscription> {
    const response = await this.request({ type: 'subscribe-share', token });
    if (response.type !== 'share-subscribed') {
      throw new Error((response['error'] as string) ?? 'subscribe-share failed');
    }
    return {
      share_id: response['share_id'] as string,
      entity_type: response['entity_type'] as string,
      entity_id: response['entity_id'] as string,
    };
  }

  /** Detach a share from this connection (e.g. the shared view was closed). */
  async unsubscribeShare(shareId: string): Promise<void> {
    await this.request({ type: 'unsubscribe-share', share_id: shareId });
  }

  private request(
    request: Record<string, unknown>,
  ): Promise<Record<string, unknown> & { type: string }> {
    if (!this.socket$) {
      return Promise.reject(new Error('Not connected'));
    }
    const id = `req-${++this.requestCounter}-${Date.now()}`;
    const promise = new Promise<Record<string, unknown> & { type: string }>(resolve => {
      this.pendingRequests.set(id, { resolve });
    });
    this.socket$.next({ type: 'request', id, request });
    return promise;
  }

  private route(msg: ErnoWsMessage): void {
    if (msg.type === 'response' && msg.id && msg.response) {
      this.pendingRequests.get(msg.id)?.resolve(msg.response);
      this.pendingRequests.delete(msg.id);
      return;
    }

    if (msg.type !== 'broadcast' || !msg.broadcast) return;
    const broadcast = msg.broadcast;

    if (broadcast.type === 'share-granted' || broadcast.type === 'share-revoked') {
      this.shareEventsSubject$.next({
        type: broadcast.type,
        share_id: broadcast['share_id'] as string,
        entity_type: broadcast['entity_type'] as string | undefined,
        entity_id: broadcast['entity_id'] as string | undefined,
      });
      return;
    }

    if (broadcast.type === 'application' && broadcast['entity_type']) {
      const snapshot = broadcast['snapshot'] as { deleted_at?: string | null } | null;
      this.syncEvents$.next({
        entity: broadcast['entity_type'] as string,
        id: broadcast['entity_id'] as string,
        sync_seq: broadcast['sync_seq'] as number,
        deleted: broadcast['operation'] === 'delete' || snapshot?.deleted_at != null,
        data: snapshot,
      });
    }
  }

  ngOnDestroy(): void {
    this.disconnect();
    this.lifecycleSubs.unsubscribe();
    this.syncEvents$.complete();
    this.shareEventsSubject$.complete();
    this.connectedSubject$.complete();
  }

  /**
   * Test seam: overridable factory so specs can inject a fake socket.
   *
   * `handlers` carries the open/close callbacks that back `connected$`; a fake
   * socket has no lifecycle of its own, so specs invoke them by hand.
   */
  protected createSocket(
    url: string,
    handlers: { onOpen: () => void; onClose: () => void },
  ): WebSocketSubject<ErnoWsMessage> {
    return webSocket<ErnoWsMessage>({
      url,
      openObserver: { next: () => handlers.onOpen() },
      closeObserver: { next: () => handlers.onClose() },
    });
  }

  private openSocket(): void {
    const gen = ++this.openGeneration;
    this.clearReconnectTimer();
    this.teardownSocket();

    // The handshake puts the JWT in the URL, so it cannot ride the HTTP
    // interceptor's 401 refresh. Wait for a live token before connecting.
    if (jwtAccessTokenExpired(this.auth.accessToken) && this.auth.refreshToken) {
      void this.openSocketAfterRefresh(gen);
      return;
    }
    this.bindSocket();
  }

  private async openSocketAfterRefresh(gen: number): Promise<void> {
    try {
      await firstValueFrom(this.auth.refresh());
    } catch {
      // Non-fatal refresh errors leave the stale token; bindSocket still
      // tries. A 401 from refresh is a dead session — connect anonymously.
    }
    if (gen !== this.openGeneration) return;
    if (!this.shouldBeConnected || this.suspended) return;
    this.bindSocket();
  }

  private bindSocket(): void {
    const token = this.auth.accessToken;
    const url = token ? `${this.config.wsUrl}?token=${token}` : this.config.wsUrl;

    this.socket$ = this.createSocket(url, {
      onOpen: () => this.connectedSubject$.next(true),
      onClose: () => this.connectedSubject$.next(false),
    });
    this.socketSub = this.socket$.subscribe({
      next: msg => this.route(msg),
      error: () => this.handleSocketClosed(),
      complete: () => this.handleSocketClosed(),
    });
  }

  private handleSocketClosed(): void {
    this.socket$ = null;
    this.socketSub = null;
    this.connectedSubject$.next(false);
    if (this.shouldBeConnected && !this.suspended) {
      this.scheduleReconnect();
    }
  }

  private scheduleReconnect(): void {
    this.clearReconnectTimer();
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.shouldBeConnected && !this.suspended) {
        this.openSocket();
      }
    }, RECONNECT_DELAY_MS);
  }

  private onAppStateChange(state: ErnoAppState): void {
    if (state === 'background') {
      this.backgroundSuspended = true;
      this.clearReconnectTimer();
      this.teardownSocket();
      return;
    }
    if (!this.backgroundSuspended) return;
    this.backgroundSuspended = false;
    this.tryResumeSocket();
  }

  private onNetworkChange(connected: boolean): void {
    if (!connected) {
      this.offlineSuspended = true;
      this.clearReconnectTimer();
      this.teardownSocket();
      return;
    }
    if (!this.offlineSuspended) return;
    this.offlineSuspended = false;
    this.tryResumeSocket();
  }

  /** Reopen the socket if the consumer still wants it and nothing suspends it. */
  private tryResumeSocket(): void {
    if (this.shouldBeConnected && !this.suspended && !this.socket$) {
      this.openSocket();
    }
  }

  /** Tears the socket down without firing the `complete` callback. */
  private teardownSocket(): void {
    this.socketSub?.unsubscribe();
    this.socketSub = null;
    this.socket$ = null;
    // Unsubscribing never fires closeObserver, so report the drop here.
    this.connectedSubject$.next(false);
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }
}
