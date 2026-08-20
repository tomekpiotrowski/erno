import { Injectable } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Subject } from 'rxjs';
import type { WebSocketSubject } from 'rxjs/webSocket';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoAuthService } from '../auth/erno-auth.service';
import { ErnoAppStateService } from '../app-state/erno-app-state.service';
import { ErnoNetworkService } from '../network/erno-network.service';
import { ErnoRealtimeService, SyncPushEvent } from './erno-realtime.service';

@Injectable()
class TestableRealtimeService extends ErnoRealtimeService {
  sockets: Subject<unknown>[] = [];
  urls: string[] = [];

  // ErnoWsMessage is private on the base; cast through unknown for the test seam.
  protected override createSocket(url: string): WebSocketSubject<any> {
    this.urls.push(url);
    const socket = new Subject<unknown>();
    this.sockets.push(socket);
    return socket as unknown as WebSocketSubject<any>;
  }

  /** The currently-open socket, or undefined if none. */
  get latest(): Subject<unknown> | undefined {
    return this.sockets[this.sockets.length - 1];
  }
}

describe('ErnoRealtimeService', () => {
  let service: TestableRealtimeService;
  let appState: ErnoAppStateService;
  let network: ErnoNetworkService;

  beforeEach(() => {
    vi.useFakeTimers();
    TestBed.configureTestingModule({
      providers: [
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://api', wsUrl: 'ws://api/ws' } },
        { provide: ErnoAuthService, useValue: { accessToken: 'tok' } },
        ErnoAppStateService,
        ErnoNetworkService,
        { provide: ErnoRealtimeService, useClass: TestableRealtimeService },
      ],
    });
    appState = TestBed.inject(ErnoAppStateService);
    network = TestBed.inject(ErnoNetworkService);
    service = TestBed.inject(ErnoRealtimeService) as unknown as TestableRealtimeService;
  });

  afterEach(() => vi.useRealTimers());

  it('opens a socket with the auth token on connect', () => {
    service.connect();
    expect(service.sockets.length).toBe(1);
    expect(service.urls[0]).toBe('ws://api/ws?token=tok');
  });

  it('forwards incoming messages to events$', () => {
    const received: SyncPushEvent[] = [];
    service.events$.subscribe((e) => received.push(e));
    service.connect();

    service.latest!.next({
      type: 'broadcast',
      broadcast: {
        type: 'application',
        entity_type: 'todo',
        entity_id: '1',
        sync_seq: 5,
        operation: 'update',
        snapshot: { title: 'hi' },
      },
    });

    expect(received).toEqual([
      {
        entity: 'todo',
        id: '1',
        sync_seq: 5,
        deleted: false,
        data: { title: 'hi' },
      },
    ]);
  });

  it('reconnects 3s after the server closes the socket', () => {
    service.connect();
    service.latest!.complete();

    expect(service.sockets.length).toBe(1);
    vi.advanceTimersByTime(3000);
    expect(service.sockets.length).toBe(2);
  });

  it('does not reconnect after an explicit disconnect', () => {
    service.connect();
    service.disconnect();

    vi.advanceTimersByTime(4000);
    expect(service.sockets.length).toBe(1);
  });

  it('tears down the socket on background without scheduling a reconnect', () => {
    service.connect();
    appState.notifyStateChange('background');

    vi.advanceTimersByTime(4000);
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

    vi.advanceTimersByTime(4000);
    expect(service.sockets.length).toBe(1);
  });

  it('tears down the socket when offline without scheduling a reconnect', () => {
    service.connect();
    network.notifyStatusChange(false);

    vi.advanceTimersByTime(4000);
    expect(service.sockets.length).toBe(1);
  });

  it('reconnects when connectivity returns if it had been connected', () => {
    service.connect();
    network.notifyStatusChange(false);
    network.notifyStatusChange(true);

    expect(service.sockets.length).toBe(2);
  });

  it('does not open the socket when connect is called while offline', () => {
    network.notifyStatusChange(false);
    service.connect();
    expect(service.sockets.length).toBe(0);

    network.notifyStatusChange(true);
    expect(service.sockets.length).toBe(1);
  });

  it('does not reconnect when going offline during the reconnect window', () => {
    service.connect();
    service.latest!.complete();
    network.notifyStatusChange(false);

    vi.advanceTimersByTime(4000);
    expect(service.sockets.length).toBe(1);
  });

  it('stays closed when online while still backgrounded', () => {
    service.connect();
    appState.notifyStateChange('background');
    network.notifyStatusChange(false);
    network.notifyStatusChange(true);

    expect(service.sockets.length).toBe(1);
    vi.advanceTimersByTime(4000);
    expect(service.sockets.length).toBe(1);
  });
});
