import { WritableSignal, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject, of, Subject } from 'rxjs';
import { ERNO_CONFIG } from '../erno.config';
import { AuthUser, ErnoAuthService } from '../auth/erno-auth.service';
import { ErnoNetworkService } from '../network/erno-network.service';
import { ErnoRealtimeService, SyncPushEvent } from '../realtime/erno-realtime.service';
import { ErnoSyncService, SyncEntityInfo, SyncStatus } from '../sync/erno-sync.service';
import { ErnoDevMailService, MockEmail } from './erno-dev-mail.service';
import { DevJob, ErnoDevJobsService } from './erno-dev-jobs.service';
import { ErnoDevtoolsComponent } from './erno-devtools.component';
import { ErnoDevtoolsRegistry } from './erno-devtools.registry';
import Dexie from 'dexie';

function email(partial: Partial<MockEmail> & Pick<MockEmail, 'id'>): MockEmail {
  return {
    to: 'ada@example.com',
    from: 'app@example.com',
    subject: 'Hello',
    body_html: '<p>Hi</p>',
    body_text: 'Hi',
    created_at: '2026-08-25T12:00:00',
    ...partial,
  };
}

function job(partial: Partial<DevJob> & Pick<DevJob, 'id' | 'type'>): DevJob {
  return {
    arguments: {},
    status: 'completed',
    retry_count: 0,
    next_execution_at: null,
    created_at: '2026-08-25T12:00:00',
    updated_at: '2026-08-25T12:00:00',
    executions: [],
    ...partial,
  };
}

describe('ErnoDevtoolsComponent', () => {
  let fixture: ComponentFixture<ErnoDevtoolsComponent>;
  let component: ErnoDevtoolsComponent;
  let connected$: BehaviorSubject<boolean>;
  let status$: BehaviorSubject<SyncStatus>;
  let lastError$: BehaviorSubject<string | null>;
  let pushEvents$: Subject<SyncPushEvent>;
  let mailList$: Subject<MockEmail[]>;
  let jobsList$: Subject<DevJob[]>;
  let currentUser: WritableSignal<AuthUser | null>;
  let pullDelta: ReturnType<typeof vi.fn>;
  let entities: ReturnType<typeof vi.fn>;
  let resetCursor: ReturnType<typeof vi.fn>;
  let retry: ReturnType<typeof vi.fn>;
  let clearMail: ReturnType<typeof vi.fn>;
  let clearJobs: ReturnType<typeof vi.fn>;
  let open: ReturnType<typeof vi.fn>;
  let login: ReturnType<typeof vi.fn>;
  let logout: ReturnType<typeof vi.fn>;
  let refresh: ReturnType<typeof vi.fn>;
  let verifyEmail: ReturnType<typeof vi.fn>;
  let wipeSyncMeta: ReturnType<typeof vi.fn>;
  let notifyStatusChange: ReturnType<typeof vi.fn>;
  let networkConnected$: BehaviorSubject<boolean>;
  let authStub: {
    currentUser: WritableSignal<AuthUser | null>;
    accessToken: string | null;
    refreshToken: string | null;
    login: ReturnType<typeof vi.fn>;
    logout: ReturnType<typeof vi.fn>;
    refresh: ReturnType<typeof vi.fn>;
    verifyEmail: ReturnType<typeof vi.fn>;
  };

  beforeEach(async () => {
    connected$ = new BehaviorSubject(true);
    networkConnected$ = new BehaviorSubject(true);
    notifyStatusChange = vi.fn().mockName('notifyStatusChange').mockImplementation((connected: boolean) => {
      networkConnected$.next(connected);
    });
    status$ = new BehaviorSubject<SyncStatus>('synced');
    lastError$ = new BehaviorSubject<string | null>(null);
    pushEvents$ = new Subject<SyncPushEvent>();
    currentUser = signal<AuthUser | null>(null);
    mailList$ = new Subject();
    jobsList$ = new Subject();
    pullDelta = vi.fn().mockName('pullDelta').mockResolvedValue(undefined);
    const entity: SyncEntityInfo = {
      entity: 'todos',
      deltaPath: '/api/todos/sync',
      lastSyncSeq: 4,
      lastPullAt: Date.now(),
      lastError: null,
    };
    entities = vi.fn().mockName('entities').mockResolvedValue([entity]);
    resetCursor = vi.fn().mockName('resetCursor').mockResolvedValue(undefined);
    retry = vi.fn().mockName('retry').mockReturnValue(of(undefined));
    clearMail = vi.fn().mockName('clearMail').mockReturnValue(of(undefined));
    clearJobs = vi.fn().mockName('clearJobs').mockReturnValue(of(undefined));
    login = vi.fn().mockName('login').mockReturnValue(of({
      access_token: 'access',
      refresh_token: 'refresh',
      user: { id: 'user-1', email: 'dev@example.com' },
    }));
    logout = vi.fn().mockName('logout').mockReturnValue(of(undefined));
    refresh = vi.fn().mockName('refresh').mockReturnValue(of({
      access_token: 'access-2',
      refresh_token: 'refresh-2',
      user: { id: 'user-1', email: 'dev@example.com' },
    }));
    verifyEmail = vi.fn().mockName('verifyEmail').mockReturnValue(of({
      access_token: 'access',
      refresh_token: 'refresh',
      user: { id: 'user-1', email: 'dev@example.com' },
    }));
    open = vi.fn().mockName('open');
    wipeSyncMeta = vi.fn().mockName('wipeSyncMeta').mockResolvedValue(undefined);
    vi.stubGlobal('open', open);
    sessionStorage.clear();
    localStorage.clear();

    const ernoDb = {
      name: 'erno',
      tables: [
        {
          name: 'syncMeta',
          count: () => Promise.resolve(1),
          limit: () => ({ toArray: () => Promise.resolve([{ entity: 'todos', lastSyncSeq: 4 }]) }),
          toArray: () => Promise.resolve([{ entity: 'todos', lastSyncSeq: 4 }]),
          clear: wipeSyncMeta,
        },
        {
          name: 'pendingMutations',
          count: () => Promise.resolve(0),
          limit: () => ({ toArray: () => Promise.resolve([]) }),
          toArray: () => Promise.resolve([]),
          clear: vi.fn().mockResolvedValue(undefined),
        },
      ],
    };

    authStub = {
      currentUser,
      accessToken: null,
      refreshToken: null,
      login,
      logout,
      refresh,
      verifyEmail,
    };

    await TestBed.configureTestingModule({
      imports: [ErnoDevtoolsComponent],
      providers: [
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://localhost:3000', wsUrl: 'ws://localhost:3000/ws' } },
        {
          provide: ErnoSyncService,
          useValue: {
            status$: status$.asObservable(),
            lastError$: lastError$.asObservable(),
            lastError: null,
            isStarted: true,
            pullDelta,
            entities,
            resetCursor,
          },
        },
        {
          provide: ErnoRealtimeService,
          useValue: {
            connected$: connected$.asObservable(),
            events$: pushEvents$.asObservable(),
          },
        },
        { provide: ErnoAuthService, useValue: authStub },
        {
          provide: ErnoNetworkService,
          useValue: {
            connected$: networkConnected$.asObservable(),
            notifyStatusChange,
          },
        },
        ErnoDevtoolsRegistry,
        {
          provide: ErnoDevMailService,
          useValue: {
            list: () => mailList$.asObservable(),
            delete: () => of(undefined),
            clear: clearMail,
            previewUrl: (id: string) => `http://localhost:3000/dev/emails/${id}/preview`,
          },
        },
        {
          provide: ErnoDevJobsService,
          useValue: {
            list: () => jobsList$.asObservable(),
            retry,
            clear: clearJobs,
          },
        },
        {
          provide: HttpClient,
          useValue: { get: () => of('ok') },
        },
      ],
    }).compileComponents();

    TestBed.inject(ErnoDevtoolsRegistry).register(ernoDb as unknown as Dexie);

    fixture = TestBed.createComponent(ErnoDevtoolsComponent);
    component = fixture.componentInstance;
    fixture.detectChanges();
  });

  afterEach(() => {
    fixture.destroy();
    vi.unstubAllGlobals();
    sessionStorage.clear();
    localStorage.clear();
  });

  function clickTab(label: string): void {
    const btn = [...fixture.nativeElement.querySelectorAll('.tab')].find((b: HTMLButtonElement) =>
      b.textContent?.includes(label),
    ) as HTMLButtonElement;
    btn.click();
    fixture.detectChanges();
  }

  function flushLists(emails: MockEmail[] = [], jobs: DevJob[] = []): void {
    mailList$.next(emails);
    jobsList$.next(jobs);
    fixture.detectChanges();
  }

  it('renders the Nocturne panel with status rows', () => {
    flushLists();
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('Erno Devtools');
    expect(text).toContain('websocket');
    expect(text).toContain('connected');
    expect(text).toContain('network');
    expect(text).toContain('online');
    expect(text).toContain('localhost:3000');
    expect(text).toContain('Re-sync');
  });

  it('collapses to a pill and reopens', () => {
    flushLists();
    const collapse = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.title === 'collapse',
    ) as HTMLButtonElement;
    collapse.click();
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).toContain('Erno Devtools');
    expect(fixture.nativeElement.querySelector('.pill')).toBeTruthy();
    expect(fixture.nativeElement.querySelector('.panel')).toBeNull();

    fixture.nativeElement.querySelector('.pill').click();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('.panel')).toBeTruthy();
  });

  it('switches to emails and opens a preview tab', () => {
    flushLists([email({ id: 'm1', subject: 'Reset your password' })]);
    clickTab('Emails');
    expect(fixture.nativeElement.textContent).toContain('Reset your password');
    expect(fixture.nativeElement.textContent).not.toContain('Outbox empty');

    fixture.nativeElement.querySelector('.erow').click();
    expect(open).toHaveBeenCalledWith(
      'http://localhost:3000/dev/emails/m1/preview',
      '_blank',
      'noopener',
    );
  });

  it('shows the empty outbox copy', () => {
    flushLists([]);
    clickTab('Emails');
    expect(fixture.nativeElement.textContent).toContain('Outbox empty');
  });

  it('groups jobs, expands a failed kind, and retries', () => {
    flushLists(
      [],
      [
        job({
          id: 'j-fail',
          type: 'charge_pending_orders',
          status: 'failed',
          executions: [
            {
              id: 'ex1',
              result: 'failed',
              execution_time_ms: 240,
              failure_reason: 'PoolTimedOut',
              started_at: '2026-08-25T12:00:00',
              finished_at: '2026-08-25T12:00:00',
            },
          ],
        }),
        job({ id: 'j-ok', type: 'expire_sessions', status: 'completed' }),
      ],
    );
    clickTab('Jobs');
    expect(fixture.nativeElement.textContent).toContain('charge_pending_orders');
    expect(fixture.nativeElement.textContent).toContain('Failed');

    const row = [...fixture.nativeElement.querySelectorAll('.jrow')].find((el: HTMLElement) =>
      el.textContent?.includes('charge_pending_orders'),
    ) as HTMLElement;
    row.click();
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).toContain('PoolTimedOut');

    const retryBtn = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'retry',
    ) as HTMLButtonElement;
    retryBtn.click();
    expect(retry).toHaveBeenCalledWith('j-fail');
  });

  it('force re-syncs through the sync service', async () => {
    flushLists();
    const btn = [...fixture.nativeElement.querySelectorAll('button')].find((b: HTMLButtonElement) =>
      b.textContent?.includes('Re-sync'),
    ) as HTMLButtonElement;
    btn.click();
    expect(pullDelta).toHaveBeenCalled();
    await Promise.resolve();
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).toContain('caught up');
  });

  it('clears the outbox from the emails tab', () => {
    flushLists([email({ id: 'm1' })]);
    clickTab('Emails');
    const clear = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'Clear all',
    ) as HTMLButtonElement;
    clear.click();
    expect(clearMail).toHaveBeenCalled();
  });

  it('renders the Auth tab signed out with seed credentials', () => {
    flushLists();
    clickTab('Auth');
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('signed out');
    expect(text).toContain('sign in');
    const email = fixture.nativeElement.querySelector('input[placeholder="email"]') as HTMLInputElement;
    const password = fixture.nativeElement.querySelector('input[placeholder="password"]') as HTMLInputElement;
    expect(email.value).toBe('dev@example.com');
    expect(password.value).toBe('password');
  });

  it('signs in through the auth service with the seed form', () => {
    flushLists();
    clickTab('Auth');
    const signIn = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'sign in',
    ) as HTMLButtonElement;
    signIn.click();
    expect(login).toHaveBeenCalledWith('dev@example.com', 'password');
  });

  it('drops the access token from sessionStorage', () => {
    sessionStorage.setItem('erno_access_token', 'keep-me-not');
    authStub.accessToken = 'keep-me-not';
    authStub.refreshToken = 'refresh';
    currentUser.set({ id: 'user-1', email: 'dev@example.com' });
    flushLists();
    clickTab('Auth');
    const drop = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'drop access',
    ) as HTMLButtonElement;
    drop.click();
    fixture.detectChanges();
    expect(sessionStorage.getItem('erno_access_token')).toBeNull();
    expect(fixture.nativeElement.textContent).toContain('access token dropped');
  });

  it('verifies from a verification email without opening the preview', () => {
    flushLists([
      email({
        id: 'm-verify',
        subject: 'Verify your email',
        body_html: '<p><a href="http://localhost:4200/verify-email?token=tok-9">here</a></p>',
      }),
    ]);
    clickTab('Emails');
    const verify = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'verify',
    ) as HTMLButtonElement;
    expect(verify).toBeTruthy();
    verify.click();
    expect(verifyEmail).toHaveBeenCalledWith('tok-9');
    expect(open).not.toHaveBeenCalled();
  });

  it('lists a registered entity on the Sync tab and resets its cursor', async () => {
    flushLists();
    clickTab('Sync');
    await Promise.resolve();
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).toContain('todos');
    expect(fixture.nativeElement.textContent).toContain('seq 4');

    const reset = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'reset cursor',
    ) as HTMLButtonElement;
    reset.click();
    await Promise.resolve();
    fixture.detectChanges();
    expect(resetCursor).toHaveBeenCalledWith('todos');
  });

  it('keeps the push log across tab switches and shows a receive clock', async () => {
    flushLists();
    pushEvents$.next({
      entity: 'todos',
      id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
      sync_seq: 12,
      deleted: false,
      data: {},
    });
    fixture.detectChanges();
    clickTab('Emails');
    clickTab('Sync');
    await Promise.resolve();
    fixture.detectChanges();
    const text = fixture.nativeElement.textContent as string;
    expect(text).toContain('aaaaaaaa');
    expect(text).toContain('#12');
    expect(text).toContain('upsert');
    expect(text).toMatch(/\d{2}:\d{2}:\d{2}/);
  });

  it('simulates offline through the network service', () => {
    flushLists();
    const btn = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'simulate offline',
    ) as HTMLButtonElement;
    btn.click();
    fixture.detectChanges();
    expect(notifyStatusChange).toHaveBeenCalledWith(false);
    expect(fixture.nativeElement.textContent).toContain('offline');
    expect(fixture.nativeElement.textContent).toContain('go online');
  });

  it('shows the last sync error on the Status row', () => {
    lastError$.next('todos: 500 Server Error');
    status$.next('error');
    flushLists();
    expect(fixture.nativeElement.textContent).toContain('todos: 500 Server Error');
  });

  it('lists Dexie tables on the Data tab and wipes one', async () => {
    flushLists();
    clickTab('Data');
    await new Promise<void>(resolve => setTimeout(resolve, 0));
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).toContain('erno');
    expect(fixture.nativeElement.textContent).toContain('syncMeta');
    expect(fixture.nativeElement.textContent).toContain('pendingMutations');

    const syncMetaRow = [...fixture.nativeElement.querySelectorAll('.jrow')].find((el: HTMLElement) =>
      el.textContent?.includes('syncMeta'),
    ) as HTMLElement;
    syncMetaRow.click();
    fixture.detectChanges();
    expect(fixture.nativeElement.textContent).toContain('todos');

    const wipe = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'wipe',
    ) as HTMLButtonElement;
    wipe.click();
    await new Promise<void>(resolve => setTimeout(resolve, 0));
    fixture.detectChanges();
    expect(wipeSyncMeta).toHaveBeenCalled();
  });
});
