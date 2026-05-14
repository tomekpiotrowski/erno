import { inject, Injectable, OnDestroy } from '@angular/core';
import { Observable, Subject, timer } from 'rxjs';
import { webSocket, WebSocketSubject } from 'rxjs/webSocket';
import { retryWhen, delay, tap } from 'rxjs/operators';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoAuthService } from '../auth/erno-auth.service';

export interface SyncPushEvent {
  entity: string;
  id: string;
  sync_seq: number;
  deleted: boolean;
}

@Injectable()
export class ErnoRealtimeService implements OnDestroy {
  private config = inject(ERNO_CONFIG);
  private auth = inject(ErnoAuthService);

  private socket$: WebSocketSubject<SyncPushEvent> | null = null;
  private messages$ = new Subject<SyncPushEvent>();

  get events$(): Observable<SyncPushEvent> {
    return this.messages$.asObservable();
  }

  connect(): void {
    const token = this.auth.accessToken;
    if (!token) return;

    this.socket$ = webSocket<SyncPushEvent>(`${this.config.wsUrl}?token=${token}`);
    this.socket$.subscribe({
      next: msg => this.messages$.next(msg),
      error: () => setTimeout(() => this.connect(), 3000),
      complete: () => setTimeout(() => this.connect(), 3000),
    });
  }

  disconnect(): void {
    this.socket$?.complete();
    this.socket$ = null;
  }

  ngOnDestroy(): void {
    this.disconnect();
    this.messages$.complete();
  }
}
