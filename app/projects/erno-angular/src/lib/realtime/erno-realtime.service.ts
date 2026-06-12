import { Inject, Injectable, OnDestroy } from '@angular/core';
import { Observable, Subject, Subscription } from 'rxjs';
import { webSocket, WebSocketSubject } from 'rxjs/webSocket';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { ErnoAuthService } from '../auth/erno-auth.service';
import { ErnoAppState, ErnoAppStateService } from '../app-state/erno-app-state.service';

export interface SyncPushEvent {
  entity: string;
  id: string;
  sync_seq: number;
  deleted: boolean;
}

const RECONNECT_DELAY_MS = 3000;

@Injectable()
export class ErnoRealtimeService implements OnDestroy {
  private socket$: WebSocketSubject<SyncPushEvent> | null = null;
  private socketSub: Subscription | null = null;
  private messages$ = new Subject<SyncPushEvent>();

  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  /** Whether the consumer wants a connection (set by connect/disconnect). */
  private shouldBeConnected = false;
  /** Whether the connection is paused because the app is backgrounded. */
  private suspended = false;
  private appStateSub: Subscription;

  constructor(
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
    private auth: ErnoAuthService,
    private appState: ErnoAppStateService,
  ) {
    this.appStateSub = this.appState.state$.subscribe(state => this.onAppStateChange(state));
  }

  get events$(): Observable<SyncPushEvent> {
    return this.messages$.asObservable();
  }

  connect(): void {
    this.shouldBeConnected = true;
    if (this.appState.state === 'background') {
      this.suspended = true;
      return;
    }
    this.openSocket();
  }

  disconnect(): void {
    this.shouldBeConnected = false;
    this.clearReconnectTimer();
    this.teardownSocket();
  }

  ngOnDestroy(): void {
    this.disconnect();
    this.appStateSub.unsubscribe();
    this.messages$.complete();
  }

  /** Test seam: overridable factory so specs can inject a fake socket. */
  protected createSocket(url: string): WebSocketSubject<SyncPushEvent> {
    return webSocket<SyncPushEvent>(url);
  }

  private openSocket(): void {
    this.clearReconnectTimer();
    this.teardownSocket();

    const token = this.auth.accessToken;
    // TODO: retry once a token becomes available rather than waiting for the
    // next foreground resume.
    if (!token) return;

    this.socket$ = this.createSocket(`${this.config.wsUrl}?token=${token}`);
    this.socketSub = this.socket$.subscribe({
      next: msg => this.messages$.next(msg),
      error: () => this.handleSocketClosed(),
      complete: () => this.handleSocketClosed(),
    });
  }

  private handleSocketClosed(): void {
    this.socket$ = null;
    this.socketSub = null;
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
      this.suspended = true;
      this.clearReconnectTimer();
      this.teardownSocket();
    } else if (this.suspended) {
      this.suspended = false;
      if (this.shouldBeConnected && !this.socket$) {
        this.openSocket();
      }
    }
  }

  /** Tears the socket down without firing the `complete` callback. */
  private teardownSocket(): void {
    this.socketSub?.unsubscribe();
    this.socketSub = null;
    this.socket$ = null;
  }

  private clearReconnectTimer(): void {
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
  }
}
