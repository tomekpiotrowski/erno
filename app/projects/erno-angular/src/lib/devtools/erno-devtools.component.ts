import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  Inject,
  isDevMode,
  OnInit,
  computed,
  inject,
  signal,
} from '@angular/core';
import { HttpClient } from '@angular/common/http';
import { takeUntilDestroyed } from '@angular/core/rxjs-interop';
import { catchError, of } from 'rxjs';
import { ERNO_CONFIG, ErnoConfig } from '../erno.config';
import { AuthUser, ErnoAuthService } from '../auth/erno-auth.service';
import { ErnoNetworkService } from '../network/erno-network.service';
import { ErnoRealtimeService } from '../realtime/erno-realtime.service';
import { ErnoSyncService, SyncStatus } from '../sync/erno-sync.service';
import { ErnoDevMailService, MockEmail } from './erno-dev-mail.service';
import { DevJob, ErnoDevJobsService } from './erno-dev-jobs.service';
import { ERNO_DEVTOOLS_STYLES } from './erno-devtools.styles';
import { ErnoDevtoolsAuthTab } from './tabs/auth-tab';
import { ErnoDevtoolsDataTab } from './tabs/data-tab';
import { ErnoDevtoolsEmailsTab } from './tabs/emails-tab';
import { ErnoDevtoolsJobsTab } from './tabs/jobs-tab';
import { ErnoDevtoolsStatusTab } from './tabs/status-tab';
import { ErnoDevtoolsSyncTab } from './tabs/sync-tab';
import {
  DevtoolsTab,
  DT_ERR,
  DT_OK,
  DT_WARN,
  JobKindFilter,
  LoggedPushEvent,
  apiHost,
  filterJobGroups,
  formatClock,
  formatUptime,
  groupJobs,
  prependPushEvent,
  syncLabel,
  syncTone,
} from './erno-devtools.util';

const VERSION = '0.0.1';
const POLL_MS = 4000;
const NOTE_MS = 3000;

@Component({
  selector: 'erno-devtools',
  imports: [
    ErnoDevtoolsStatusTab,
    ErnoDevtoolsAuthTab,
    ErnoDevtoolsSyncTab,
    ErnoDevtoolsDataTab,
    ErnoDevtoolsEmailsTab,
    ErnoDevtoolsJobsTab,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [ERNO_DEVTOOLS_STYLES],
  template: `
    @if (visible) {
      @if (open()) {
        <div
          class="panel"
          [class.tall]="tall()"
          [style.--health]="healthColor()"
          role="dialog"
          aria-label="Erno Devtools"
        >
          <div class="head">
            <span class="health"></span>
            <span class="title">Erno Devtools</span>
            <span class="ver">{{ version }}</span>
            <span class="head-acts">
              <button type="button" class="ghost" (click)="toggleDock()">
                {{ tall() ? 'short' : 'tall' }}
              </button>
              <button type="button" class="ghost icon" (click)="open.set(false)" title="collapse">
                —
              </button>
            </span>
          </div>

          <div class="tabs" role="tablist">
            @for (t of tabDefs(); track t.key) {
              <button
                type="button"
                role="tab"
                class="tab"
                [class.on]="tab() === t.key"
                [attr.aria-selected]="tab() === t.key"
                (click)="selectTab(t.key)"
              >
                <span>{{ t.label }}</span>
                @if (t.count) {
                  <span class="count" [class.err]="t.tone === 'err'" [class.accent]="t.tone === 'accent'">
                    {{ t.count }}
                  </span>
                }
              </button>
            }
          </div>
          <div class="rule"></div>

          <div class="body">
            @if (tab() === 'status') {
              <erno-devtools-status-tab
                [rows]="statusRows()"
                [syncBusy]="syncBusy()"
                [syncStatus]="syncStatus()"
                [syncHint]="syncHint()"
                [online]="online()"
                (resync)="forceSync()"
                (toggleNetwork)="toggleNetwork()"
              />
            }
            @if (tab() === 'auth') {
              <erno-devtools-auth-tab (note)="say($event)" />
            }
            @if (tab() === 'sync') {
              <erno-devtools-sync-tab
                [events]="pushLog()"
                (note)="say($event)"
                (clearLog)="pushLog.set([])"
              />
            }
            @if (tab() === 'data') {
              <erno-devtools-data-tab (note)="say($event)" />
            }
            @if (tab() === 'emails') {
              <erno-devtools-emails-tab
                [emails]="emails()"
                [unread]="unread()"
                (openEmail)="openEmail($event)"
                (deleteEmail)="deleteEmail($event)"
                (verify)="verifyEmail($event)"
                (openReset)="openReset($event)"
              />
            }
            @if (tab() === 'jobs') {
              <erno-devtools-jobs-tab
                [groups]="visibleGroups()"
                [query]="jobQuery()"
                [filter]="jobFilter()"
                [expanded]="expanded()"
                [emptyHint]="jobsEmptyHint()"
                (queryChange)="jobQuery.set($event)"
                (filterChange)="jobFilter.set($event)"
                (toggleGroup)="toggleGroup($event)"
                (retryJob)="retryJob($event)"
              />
            }
          </div>

          <div class="foot">
            <span class="fnote">{{ footNote() }}</span>
            <button type="button" class="secondary" (click)="clearAll()">Clear all</button>
          </div>
        </div>
      } @else {
        <button type="button" class="pill" [style.--health]="healthColor()" (click)="open.set(true)">
          <span class="health"></span>
          <span>Erno Devtools</span>
          <span class="pcnt">{{ pillCounts() }}</span>
        </button>
      }
    }
  `,
})
export class ErnoDevtoolsComponent implements OnInit {
  private readonly destroyRef = inject(DestroyRef);

  readonly visible = isDevMode();
  readonly version = VERSION;

  readonly open = signal(true);
  readonly tall = signal(false);
  readonly tab = signal<DevtoolsTab>('status');
  readonly emails = signal<MockEmail[]>([]);
  readonly jobs = signal<DevJob[]>([]);
  readonly expanded = signal<Set<string>>(new Set());
  readonly jobQuery = signal('');
  readonly jobFilter = signal<JobKindFilter>('all');
  readonly unread = signal<Set<string>>(new Set());
  readonly user = signal<AuthUser | null>(null);
  readonly wsConnected = signal(false);
  readonly syncStatus = signal<SyncStatus>('idle');
  readonly lastSyncError = signal<string | null>(null);
  readonly syncBusy = signal(false);
  readonly apiReady = signal<boolean | null>(null);
  readonly online = signal(true);
  readonly simulatingOffline = signal(false);
  readonly pushLog = signal<LoggedPushEvent[]>([]);
  readonly note = signal('');
  readonly connectedSince = signal<number | null>(null);
  readonly syncAt = signal<number | null>(null);

  private seenEmailIds = new Set<string>();
  private emailsPrimed = false;
  private noteTimer: ReturnType<typeof setTimeout> | null = null;
  private pollTimer: ReturnType<typeof setInterval> | null = null;

  readonly groups = computed(() => groupJobs(this.jobs()));
  readonly visibleGroups = computed(() =>
    filterJobGroups(this.groups(), this.jobQuery(), this.jobFilter()),
  );
  readonly failing = computed(() => this.jobs().filter(j => j.status === 'failed').length);
  readonly running = computed(
    () => this.jobs().filter(j => j.status === 'running' || j.status === 'pending_retry').length,
  );
  readonly unreadCount = computed(() => this.unread().size);

  readonly healthColor = computed(() => {
    if (this.syncStatus() === 'error' || this.failing() > 0) return DT_ERR;
    if (this.running() > 0 || this.syncStatus() === 'syncing') return DT_WARN;
    return DT_OK;
  });

  readonly tabDefs = computed(() => {
    const emails = this.emails().length;
    const runs = this.jobs().length;
    return [
      {
        key: 'status' as const,
        label: 'Status',
        count: this.syncStatus() === 'error' ? '!' : null,
        tone: 'err' as const,
      },
      {
        key: 'auth' as const,
        label: 'Auth',
        count: this.user() ? 'in' : null,
        tone: 'muted' as const,
      },
      {
        key: 'sync' as const,
        label: 'Sync',
        count: this.syncStatus() === 'error' ? '!' : null,
        tone: 'err' as const,
      },
      {
        key: 'data' as const,
        label: 'Data',
        count: null,
        tone: 'muted' as const,
      },
      {
        key: 'emails' as const,
        label: 'Emails',
        count: emails ? String(emails) : null,
        tone: this.unreadCount() ? ('accent' as const) : ('muted' as const),
      },
      {
        key: 'jobs' as const,
        label: 'Jobs',
        count: runs ? String(runs) : null,
        tone: this.failing() ? ('err' as const) : ('muted' as const),
      },
    ];
  });

  readonly statusRows = computed(() => {
    const up = this.connectedSince();
    const upLabel = up != null ? formatUptime(up, Date.now()) : '—';
    const host = apiHost(this.config.baseUrl);
    const wsHost = this.config.wsUrl.replace(/^wss?:\/\//, '');
    const sync = this.syncStatus();
    const api = this.apiReady();
    return [
      {
        key: 'network',
        val: this.online() ? 'online' : 'offline',
        tone: this.online() ? ('ok' as const) : ('err' as const),
        meta: this.simulatingOffline() ? 'simulated' : '',
        detail: this.online() ? '' : 'sync and the socket stay down until a path returns',
      },
      {
        key: 'websocket',
        val: this.wsConnected() ? 'connected' : 'disconnected',
        tone: this.wsConnected() ? ('ok' as const) : ('err' as const),
        meta: `${wsHost} · ${upLabel}`,
        detail: '',
      },
      {
        key: 'sync',
        val: syncLabel(sync),
        tone: syncTone(sync),
        meta: this.syncAt() ? formatClock(this.syncAt()!) : '',
        detail:
          sync === 'error'
            ? this.lastSyncError() ?? 'last pull failed — force a re-sync once the API is reachable'
            : sync === 'offline'
              ? 'waiting for a network path'
              : '',
      },
      {
        key: 'api',
        val: api == null ? 'probing' : api ? 'ready' : 'unreachable',
        tone: api === false ? ('err' as const) : api ? ('ok' as const) : ('warn' as const),
        meta: host,
        detail: '',
      },
      {
        key: 'queue',
        val: `${this.running()} running · ${this.failing()} failed`,
        tone: this.failing() ? ('err' as const) : ('ok' as const),
        meta: `${this.jobs().length} held`,
        detail: '',
      },
    ];
  });

  readonly footNote = computed(() => {
    if (this.note()) return this.note();
    const up = this.connectedSince();
    const upLabel = up != null ? formatUptime(up, Date.now()) : 'down';
    switch (this.tab()) {
      case 'auth':
        return this.user() ? `${this.user()!.email} · session live` : 'signed out';
      case 'sync':
        return this.lastSyncError()
          ? this.lastSyncError()!
          : `${this.syncStatus()} · ${this.syncAt() ? formatClock(this.syncAt()!) : 'never pulled'}`;
      case 'data':
        return 'local IndexedDB · wipe is local only';
      case 'emails':
        return this.emails().length
          ? `${this.emails().length} held · ${this.unreadCount()} unread · nothing left the machine`
          : 'nothing held';
      case 'jobs':
        return `${this.visibleGroups().length} of ${this.groups().length} kinds · ${this.jobs().length} runs · ${this.failing()} failing`;
      default:
        return `ws ${this.wsConnected() ? 'up ' + upLabel : 'down'} · ${this.jobs().length} runs · ${this.emails().length} mails held`;
    }
  });

  readonly pillCounts = computed(() => {
    const bits: string[] = [];
    if (this.unreadCount()) bits.push(`${this.unreadCount()} mail`);
    if (this.failing()) bits.push(`${this.failing()} failed`);
    else bits.push(`${this.jobs().length} runs`);
    return bits.join(' · ');
  });

  readonly syncHint = computed(() => {
    if (this.note()) return this.note();
    if (this.syncStatus() === 'error') return 'pulls missed changes, then reconnects';
    const at = this.syncAt();
    return at ? `last synced ${formatClock(at)}` : '';
  });

  readonly jobsEmptyHint = computed(() =>
    this.jobs().length
      ? `${this.groups().length} kinds held · ${this.jobs().length} runs`
      : 'history cleared',
  );

  constructor(
    private sync: ErnoSyncService,
    private realtime: ErnoRealtimeService,
    private mailService: ErnoDevMailService,
    private jobsService: ErnoDevJobsService,
    private auth: ErnoAuthService,
    private network: ErnoNetworkService,
    private http: HttpClient,
    @Inject(ERNO_CONFIG) private config: ErnoConfig,
  ) {}

  ngOnInit(): void {
    if (!this.visible) return;
    this.auth.currentUser$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(user => {
      this.user.set(user);
    });
    this.network.connected$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(connected => {
      this.online.set(connected);
      if (connected) this.simulatingOffline.set(false);
    });
    this.realtime.connected$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(connected => {
      this.wsConnected.set(connected);
      if (connected) this.connectedSince.set(Date.now());
    });
    this.realtime.events$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(event => {
      this.pushLog.update(list => prependPushEvent(list, event));
    });
    this.sync.status$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(status => {
      this.syncStatus.set(status);
      this.syncAt.set(Date.now());
    });
    this.sync.lastError$.pipe(takeUntilDestroyed(this.destroyRef)).subscribe(err => {
      this.lastSyncError.set(err);
    });
    this.refresh();
    this.pollTimer = setInterval(() => this.refresh(), POLL_MS);
    this.destroyRef.onDestroy(() => {
      if (this.pollTimer) clearInterval(this.pollTimer);
      if (this.noteTimer) clearTimeout(this.noteTimer);
    });
  }

  selectTab(tab: DevtoolsTab): void {
    this.tab.set(tab);
  }

  toggleDock(): void {
    this.tall.update(v => !v);
  }

  toggleNetwork(): void {
    if (this.online()) {
      this.simulatingOffline.set(true);
      this.network.notifyStatusChange(false);
      this.say('simulating offline');
    } else {
      this.simulatingOffline.set(false);
      this.network.notifyStatusChange(true);
      this.say('network restored');
    }
  }

  forceSync(): void {
    if (this.syncBusy()) return;
    this.syncBusy.set(true);
    void this.sync.pullDelta().finally(() => {
      this.syncBusy.set(false);
      this.say(this.syncStatus() === 'error' ? 're-sync failed' : 'caught up');
    });
  }

  verifyEmail(token: string): void {
    this.auth.verifyEmail(token).subscribe({
      next: () => this.say('email verified'),
      error: () => this.say('verify failed'),
    });
  }

  openReset(url: string): void {
    window.open(url, '_blank', 'noopener');
  }

  openEmail(email: MockEmail): void {
    this.unread.update(set => {
      const next = new Set(set);
      next.delete(email.id);
      return next;
    });
    window.open(this.mailService.previewUrl(email.id), '_blank', 'noopener');
  }

  deleteEmail(id: string): void {
    this.mailService.delete(id).subscribe({
      next: () => this.emails.update(list => list.filter(e => e.id !== id)),
    });
  }

  toggleGroup(type: string): void {
    this.expanded.update(set => {
      const next = new Set(set);
      if (next.has(type)) next.delete(type);
      else next.add(type);
      return next;
    });
  }

  retryJob(id: string): void {
    this.jobsService.retry(id).subscribe({
      next: () => {
        this.say('re-enqueued');
        this.loadJobs();
      },
    });
  }

  clearAll(): void {
    const tab = this.tab();
    if (tab === 'emails') {
      this.mailService.clear().subscribe({
        next: () => {
          this.emails.set([]);
          this.unread.set(new Set());
          this.say('outbox cleared');
        },
      });
    } else if (tab === 'jobs') {
      this.jobsService.clear().subscribe({
        next: () => {
          this.jobs.set([]);
          this.expanded.set(new Set());
          this.say('job history cleared');
        },
      });
    } else {
      this.say('status counters reset');
    }
  }

  private refresh(): void {
    this.loadEmails();
    this.loadJobs();
    this.probeApi();
  }

  private loadEmails(): void {
    this.mailService.list().subscribe({
      next: emails => {
        const newestFirst = [...emails].reverse();
        if (!this.emailsPrimed) {
          newestFirst.forEach(e => this.seenEmailIds.add(e.id));
          this.emailsPrimed = true;
        }
        this.unread.update(set => {
          const next = new Set(set);
          for (const e of newestFirst) {
            if (!this.seenEmailIds.has(e.id)) next.add(e.id);
            this.seenEmailIds.add(e.id);
          }
          return next;
        });
        this.emails.set(newestFirst);
      },
    });
  }

  private loadJobs(): void {
    this.jobsService.list().subscribe({
      next: jobs => {
        this.jobs.set(jobs.map(j => ({ ...j, executions: j.executions ?? [] })));
      },
    });
  }

  private probeApi(): void {
    this.http
      .get(`${this.config.baseUrl}/liveness`, { responseType: 'text' })
      .pipe(catchError(() => of(null)))
      .subscribe(body => this.apiReady.set(body !== null));
  }

  say(msg: string): void {
    if (this.noteTimer) clearTimeout(this.noteTimer);
    this.note.set(msg);
    this.noteTimer = setTimeout(() => this.note.set(''), NOTE_MS);
  }
}
