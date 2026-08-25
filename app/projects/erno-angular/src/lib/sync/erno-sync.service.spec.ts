import type { Mock } from 'vitest';
import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { Subject } from 'rxjs';
import { of } from 'rxjs';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoAuthService } from '../auth/erno-auth.service';
import { ErnoDatabaseService } from './erno-database.service';
import { ErnoRealtimeService, SyncPushEvent } from '../realtime/erno-realtime.service';
import { ErnoAppStateService } from '../app-state/erno-app-state.service';
import { ErnoNetworkService } from '../network/erno-network.service';
import { ErnoSyncService } from './erno-sync.service';

function jwtWithExp(exp: number): string {
  const payload = btoa(JSON.stringify({ exp }))
    .replace(/=+$/, '')
    .replace(/\+/g, '-')
    .replace(/\//g, '_');
  return `eyJhbGciOiJub25lIn0.${payload}.sig`;
}

/** Drains the microtask queue so async pull side effects (the HTTP request) run. */
const flush = () => new Promise<void>((resolve) => setTimeout(resolve));

const DELTA_PATH = '/api/todos/sync';
const DELTA_URL = `http://api${DELTA_PATH}`;

describe('ErnoSyncService', () => {
  let service: ErnoSyncService;
  let appState: ErnoAppStateService;
  let network: ErnoNetworkService;
  let http: HttpTestingController;
  let realtimeEvents: Subject<SyncPushEvent>;
  let realtimeConnected: Subject<boolean>;
  let connectSpy: Mock;
  let dbStub: {
    getLastSyncSeq: Mock;
    setLastSyncSeq: Mock;
  };
  let authStub: {
    accessToken: string | null;
    refreshToken: string | null;
    refresh: Mock;
  };

  beforeEach(() => {
    realtimeEvents = new Subject<SyncPushEvent>();
    realtimeConnected = new Subject<boolean>();
    connectSpy = vi.fn().mockName('connect');
    const realtimeStub = {
      events$: realtimeEvents.asObservable(),
      connected$: realtimeConnected.asObservable(),
      connect: connectSpy,
    };
    dbStub = {
      getLastSyncSeq: vi.fn().mockName('getLastSyncSeq').mockResolvedValue(0),
      setLastSyncSeq: vi.fn().mockName('setLastSyncSeq').mockResolvedValue(undefined),
    };
    authStub = {
      accessToken: 'tok',
      refreshToken: 'refresh',
      refresh: vi.fn().mockName('refresh').mockReturnValue(of({ access_token: 'tok' })),
    };

    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://api', wsUrl: 'ws://api/ws' } },
        { provide: ErnoDatabaseService, useValue: dbStub },
        { provide: ErnoRealtimeService, useValue: realtimeStub },
        { provide: ErnoAuthService, useValue: authStub },
        ErnoAppStateService,
        ErnoNetworkService,
        ErnoSyncService,
      ],
    });
    appState = TestBed.inject(ErnoAppStateService);
    network = TestBed.inject(ErnoNetworkService);
    http = TestBed.inject(HttpTestingController);
    service = TestBed.inject(ErnoSyncService);
  });

  afterEach(() => http.verify());

  function registerTodo(
    handler: (item: SyncPushEvent) => Promise<void> = () => Promise.resolve(),
  ): void {
    service.register('todos', DELTA_PATH, handler);
  }

  function emptyDelta() {
    return { items: [], next_since: 0 };
  }

  it('does not pull on resume before start()', async () => {
    appState.notifyStateChange('background');
    appState.notifyStateChange('active');
    await flush();

    http.expectNone(DELTA_URL);
  });

  it('connects and pulls a delta on start()', async () => {
    registerTodo();
    const started = service.start();
    await flush();

    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
    await started;

    expect(connectSpy).toHaveBeenCalledTimes(1);
  });

  it('only starts once when start() is called twice', async () => {
    registerTodo();

    const first = service.start();
    await flush();
    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
    await first;

    await service.start();
    await flush();

    expect(connectSpy).toHaveBeenCalledTimes(1);
    http.expectNone(DELTA_URL);
  });

  it('refreshes an expired access token before pulling', async () => {
    authStub.accessToken = jwtWithExp(Date.now() / 1000 - 60);
    registerTodo();

    const pull = service.pullDelta();
    await flush();
    expect(authStub.refresh).toHaveBeenCalled();
    await flush();
    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
    await pull;
  });

  it('pulls a delta on foreground resume after start()', async () => {
    registerTodo();
    const started = service.start();
    await flush();
    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
    await started;

    appState.notifyStateChange('background');
    appState.notifyStateChange('active');
    await flush();

    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
  });

  it('shares a single in-flight pull across concurrent callers', async () => {
    registerTodo();

    const a = service.pullDelta();
    const b = service.pullDelta();
    expect(a).toBe(b);

    await flush();
    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
    await a;
  });

  it('applies delta rows and advances the cursor to next_since', async () => {
    const applied: string[] = [];
    registerTodo(async (item) => {
      applied.push(item.id);
    });

    const pull = service.pullDelta();
    await flush();
    http
      .expectOne((r) => r.url === DELTA_URL)
      .flush({
        items: [
          { id: 'a', sync_seq: 1, deleted_at: null, title: 'one' },
          { id: 'b', sync_seq: 2, deleted_at: '2026-01-01T00:00:00', title: 'gone' },
        ],
        next_since: 2,
      });
    await pull;

    expect(applied).toEqual(['a', 'b']);
    expect(dbStub.setLastSyncSeq).toHaveBeenCalledWith('todos', 2);
  });

  it('skips pull while offline and sets status offline', async () => {
    registerTodo();
    network.notifyStatusChange(false);

    await service.pullDelta();
    await flush();

    http.expectNone(DELTA_URL);
    let status = '';
    service.status$.subscribe((s) => (status = s));
    expect(status).toBe('offline');
  });

  it('pulls a delta when connectivity returns after start()', async () => {
    registerTodo();
    const started = service.start();
    await flush();
    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
    await started;

    network.notifyStatusChange(false);
    network.notifyStatusChange(true);
    await flush();

    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
  });

  it('pulls a delta when the socket reconnects after a silent drop', async () => {
    registerTodo();
    const started = service.start();
    await flush();
    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
    await started;

    // No app-state or network event — only the socket coming back.
    realtimeConnected.next(true);
    await flush();

    http.expectOne((r) => r.url === DELTA_URL).flush(emptyDelta());
  });

  it('does not pull on reconnect before start()', async () => {
    registerTodo();
    realtimeConnected.next(true);
    await flush();

    http.expectNone((r) => r.url === DELTA_URL);
  });
});
