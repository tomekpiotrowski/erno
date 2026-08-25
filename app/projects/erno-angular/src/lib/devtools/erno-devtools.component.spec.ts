import { ComponentFixture, TestBed } from '@angular/core/testing';
import { HttpClient } from '@angular/common/http';
import { BehaviorSubject, of, Subject } from 'rxjs';
import { ERNO_CONFIG } from '../erno.config';
import { ErnoRealtimeService } from '../realtime/erno-realtime.service';
import { ErnoSyncService, SyncStatus } from '../sync/erno-sync.service';
import { ErnoDevMailService, MockEmail } from './erno-dev-mail.service';
import { DevJob, ErnoDevJobsService } from './erno-dev-jobs.service';
import { ErnoDevtoolsComponent } from './erno-devtools.component';

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
  let mailList$: Subject<MockEmail[]>;
  let jobsList$: Subject<DevJob[]>;
  let pullDelta: ReturnType<typeof vi.fn>;
  let retry: ReturnType<typeof vi.fn>;
  let clearMail: ReturnType<typeof vi.fn>;
  let clearJobs: ReturnType<typeof vi.fn>;
  let open: ReturnType<typeof vi.fn>;

  beforeEach(async () => {
    connected$ = new BehaviorSubject(true);
    status$ = new BehaviorSubject<SyncStatus>('synced');
    mailList$ = new Subject();
    jobsList$ = new Subject();
    pullDelta = vi.fn().mockName('pullDelta').mockResolvedValue(undefined);
    retry = vi.fn().mockName('retry').mockReturnValue(of(undefined));
    clearMail = vi.fn().mockName('clearMail').mockReturnValue(of(undefined));
    clearJobs = vi.fn().mockName('clearJobs').mockReturnValue(of(undefined));
    open = vi.fn().mockName('open');
    vi.stubGlobal('open', open);

    await TestBed.configureTestingModule({
      imports: [ErnoDevtoolsComponent],
      providers: [
        { provide: ERNO_CONFIG, useValue: { baseUrl: 'http://localhost:3000', wsUrl: 'ws://localhost:3000/ws' } },
        {
          provide: ErnoSyncService,
          useValue: { status$: status$.asObservable(), pullDelta },
        },
        {
          provide: ErnoRealtimeService,
          useValue: { connected$: connected$.asObservable() },
        },
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

    fixture = TestBed.createComponent(ErnoDevtoolsComponent);
    component = fixture.componentInstance;
    fixture.detectChanges();
  });

  afterEach(() => {
    fixture.destroy();
    vi.unstubAllGlobals();
  });

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
    const emailsTab = [...fixture.nativeElement.querySelectorAll('.tab')].find((b: HTMLButtonElement) =>
      b.textContent?.includes('Emails'),
    ) as HTMLButtonElement;
    emailsTab.click();
    fixture.detectChanges();
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
    const emailsTab = [...fixture.nativeElement.querySelectorAll('.tab')].find((b: HTMLButtonElement) =>
      b.textContent?.includes('Emails'),
    ) as HTMLButtonElement;
    emailsTab.click();
    fixture.detectChanges();
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
    const jobsTab = [...fixture.nativeElement.querySelectorAll('.tab')].find((b: HTMLButtonElement) =>
      b.textContent?.includes('Jobs'),
    ) as HTMLButtonElement;
    jobsTab.click();
    fixture.detectChanges();
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
    const emailsTab = [...fixture.nativeElement.querySelectorAll('.tab')].find((b: HTMLButtonElement) =>
      b.textContent?.includes('Emails'),
    ) as HTMLButtonElement;
    emailsTab.click();
    fixture.detectChanges();
    const clear = [...fixture.nativeElement.querySelectorAll('button')].find(
      (b: HTMLButtonElement) => b.textContent?.trim() === 'Clear all',
    ) as HTMLButtonElement;
    clear.click();
    expect(clearMail).toHaveBeenCalled();
  });
});
