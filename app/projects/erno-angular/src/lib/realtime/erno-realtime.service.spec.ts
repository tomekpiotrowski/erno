import { Injectable } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Subject } from 'rxjs';
import type { WebSocketSubject } from 'rxjs/webSocket';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoAuthService } from '../auth/erno-auth.service';
import { ErnoAppStateService } from '../app-state/erno-app-state.service';
import { ErnoRealtimeService, SyncPushEvent } from './erno-realtime.service';

@Injectable()
class TestableRealtimeService extends ErnoRealtimeService {
  sockets: Subject<SyncPushEvent>[] = [];
  urls: string[] = [];

  protected override createSocket(url: string): WebSocketSubject<SyncPushEvent> {
    this.urls.push(url);
    const socket = new Subject<SyncPushEvent>();
    this.sockets.push(socket);
    return socket as unknown as WebSocketSubject<SyncPushEvent>;
  }

  /** The currently-open socket, or undefined if none. */
  get latest(): Subject<SyncPushEvent> | undefined {
    return this.sockets[this.sockets.length - 1];
  }
}

describe('ErnoRealtimeService', () => {
  let service: TestableRealtimeService;
  let appState: ErnoAppStateService;

  beforeEach(() => {
    jasmine.clock().install();
    TestBed.configureTestingModule({
      providers: [
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://api', wsUrl: 'ws://api/ws' } },
        { provide: ErnoAuthService, useValue: { accessToken: 'tok' } },
        ErnoAppStateService,
        { provide: ErnoRealtimeService, useClass: TestableRealtimeService },
      ],
    });
    appState = TestBed.inject(ErnoAppStateService);
    service = TestBed.inject(ErnoRealtimeService) as TestableRealtimeService;
  });

  afterEach(() => jasmine.clock().uninstall());

  it('opens a socket with the auth token on connect', () => {
    service.connect();
    expect(service.sockets.length).toBe(1);
    expect(service.urls[0]).toBe('ws://api/ws?token=tok');
  });

  it('forwards incoming messages to events$', () => {
    const received: SyncPushEvent[] = [];
    service.events$.subscribe(e => received.push(e));
    service.connect();

    const event: SyncPushEvent = { entity: 'todo', id: '1', sync_seq: 5, deleted: false };
    service.latest!.next(event);

    expect(received).toEqual([event]);
  });

  it('reconnects 3s after the server closes the socket', () => {
    service.connect();
    service.latest!.complete();

    expect(service.sockets.length).toBe(1);
    jasmine.clock().tick(3000);
    expect(service.sockets.length).toBe(2);
  });

  it('does not reconnect after an explicit disconnect', () => {
    service.connect();
    service.disconnect();

    jasmine.clock().tick(4000);
    expect(service.sockets.length).toBe(1);
  });

  it('tears down the socket on background without scheduling a reconnect', () => {
    service.connect();
    appState.notifyStateChange('background');

    jasmine.clock().tick(4000);
    expect(service.sockets.length).toBe(1);
  });

  it('reconnects on foreground resume if it had been connected', () => {
    service.connect();
    appState.notifyStateChange('background');
    appState.notifyStateChange('active');

    expect(service.sockets.length).toBe(2);
  });

  it('does nothing on resume when never connected', () => {
    appState.notifyStateChange('background');
    appState.notifyStateChange('active');

    expect(service.sockets.length).toBe(0);
  });

  it('opens the socket on resume when connect was called while backgrounded', () => {
    appState.notifyStateChange('background');
    service.connect();
    expect(service.sockets.length).toBe(0);

    appState.notifyStateChange('active');
    expect(service.sockets.length).toBe(1);
  });

  it('does not reconnect when backgrounded during the reconnect window', () => {
    service.connect();
    service.latest!.complete();
    // reconnect scheduled; background before it fires
    appState.notifyStateChange('background');

    jasmine.clock().tick(4000);
    expect(service.sockets.length).toBe(1);
  });
});
