import { TestBed } from '@angular/core/testing';
import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { Subject } from 'rxjs';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoDatabaseService } from './erno-database.service';
import { ErnoRealtimeService, SyncPushEvent } from '../realtime/erno-realtime.service';
import { ErnoAppStateService } from '../app-state/erno-app-state.service';
import { ErnoSyncService } from './erno-sync.service';

/** Drains the microtask queue so async pull side effects (the HTTP request) run. */
const flush = () => new Promise<void>(resolve => setTimeout(resolve));

const DELTA_URL = 'http://api/api/sync/delta';

describe('ErnoSyncService', () => {
  let service: ErnoSyncService;
  let appState: ErnoAppStateService;
  let http: HttpTestingController;
  let realtimeEvents: Subject<SyncPushEvent>;
  let connectSpy: jasmine.Spy;

  beforeEach(() => {
    realtimeEvents = new Subject<SyncPushEvent>();
    connectSpy = jasmine.createSpy('connect');
    const realtimeStub = { events$: realtimeEvents.asObservable(), connect: connectSpy };
    const dbStub = {
      getLastSyncSeq: jasmine.createSpy('getLastSyncSeq').and.resolveTo(0),
      setLastSyncSeq: jasmine.createSpy('setLastSyncSeq').and.resolveTo(undefined),
    };

    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(),
        provideHttpClientTesting(),
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://api', wsUrl: 'ws://api/ws' } },
        { provide: ErnoDatabaseService, useValue: dbStub },
        { provide: ErnoRealtimeService, useValue: realtimeStub },
        ErnoAppStateService,
        ErnoSyncService,
      ],
    });
    appState = TestBed.inject(ErnoAppStateService);
    http = TestBed.inject(HttpTestingController);
    service = TestBed.inject(ErnoSyncService);
  });

  afterEach(() => http.verify());

  it('does not pull on resume before start()', async () => {
    appState.notifyStateChange('background');
    appState.notifyStateChange('active');
    await flush();

    http.expectNone(DELTA_URL);
  });

  it('connects and pulls a delta on start()', async () => {
    service.register('todo', () => Promise.resolve());
    const started = service.start();
    await flush();

    http.expectOne(r => r.url === DELTA_URL).flush([]);
    await started;

    expect(connectSpy).toHaveBeenCalledTimes(1);
  });

  it('only starts once when start() is called twice', async () => {
    service.register('todo', () => Promise.resolve());

    const first = service.start();
    await flush();
    http.expectOne(r => r.url === DELTA_URL).flush([]);
    await first;

    await service.start();
    await flush();

    expect(connectSpy).toHaveBeenCalledTimes(1);
    http.expectNone(DELTA_URL);
  });

  it('pulls a delta on foreground resume after start()', async () => {
    service.register('todo', () => Promise.resolve());
    const started = service.start();
    await flush();
    http.expectOne(r => r.url === DELTA_URL).flush([]);
    await started;

    appState.notifyStateChange('background');
    appState.notifyStateChange('active');
    await flush();

    http.expectOne(r => r.url === DELTA_URL).flush([]);
  });

  it('shares a single in-flight pull across concurrent callers', async () => {
    service.register('todo', () => Promise.resolve());

    const a = service.pullDelta();
    const b = service.pullDelta();
    expect(a).toBe(b);

    await flush();
    http.expectOne(r => r.url === DELTA_URL).flush([]);
    await a;
  });
});
