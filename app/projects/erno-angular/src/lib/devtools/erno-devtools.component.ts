import { ChangeDetectionStrategy, Component, isDevMode, OnInit, signal } from '@angular/core';
import { AsyncPipe, DatePipe, JsonPipe } from '@angular/common';
import { ErnoSyncService } from '../sync/erno-sync.service';
import { ErnoDevMailService, MockEmail } from './erno-dev-mail.service';
import { ErnoDevJobsService, DevJob } from './erno-dev-jobs.service';

type Tab = 'status' | 'emails' | 'jobs';

@Component({
  selector: 'erno-devtools',
  imports: [AsyncPipe, DatePipe, JsonPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (visible) {
      <div class="erno-devtools" [class.wide]="tab === 'emails' || tab === 'jobs'">
        <div class="header">
          <strong>Erno Devtools</strong>
          <div class="tabs">
            <button [class.active]="tab === 'status'" (click)="tab = 'status'">Status</button>
            <button [class.active]="tab === 'emails'" (click)="switchToEmails()">
              Emails@if (emails().length) {
              <span> ({{ emails().length }})</span>
            }
          </button>
          <button [class.active]="tab === 'jobs'" (click)="switchToJobs()">
            Jobs@if (jobs().length) {
            <span> ({{ jobs().length }})</span>
          }
        </button>
      </div>
    </div>
    @if (tab === 'status') {
      <div>WS: {{ wsStatus }}</div>
      <div>Sync: {{ syncStatus$ | async }}</div>
      <button (click)="forceSync()">Force re-sync</button>
    }
    @if (tab === 'emails') {
      <div class="email-toolbar">
        <button (click)="loadEmails()">↺</button>
        <button (click)="clearAll()" [disabled]="emails().length === 0">Clear all</button>
      </div>
      @if (emails().length === 0) {
        <div class="empty">No emails sent.</div>
      }
      @for (email of emails(); track email.id) {
        <div class="email-card" (click)="openEmail(email)" title="Open in a new tab">
          <div class="email-line">
            <span class="subject">{{ email.subject }}</span>
            <span class="time">{{ email.created_at | date:'HH:mm:ss' }}</span>
          </div>
          <div class="email-line">
            <span class="to">{{ email.to }}</span>
            <span class="actions">
              <span class="open-hint">open ↗</span>
              <button class="icon-btn" title="Delete"
                (click)="deleteEmail(email.id); $event.stopPropagation()">×</button>
            </span>
          </div>
        </div>
      }
    }
    @if (tab === 'jobs') {
      <div class="email-toolbar">
        <button (click)="loadJobs()">↺</button>
        <button (click)="clearJobs()" [disabled]="jobs().length === 0">Clear all</button>
      </div>
      @if (jobs().length === 0) {
        <div class="empty">No jobs.</div>
      }
      @for (job of jobs(); track job) {
        <div class="email-row">
          <div class="email-summary" (click)="toggleJob(job.id)">
            <span class="arrow">{{ expandedJob() === job.id ? '▾' : '▸' }}</span>
            <span class="subject">{{ job.type }}</span>
            <span class="to" [class]="'status-' + job.status">{{ job.status }}</span>
          </div>
          @if (expandedJob() === job.id) {
            <pre class="body-text">{{ job.arguments | json }}</pre>
            <div class="job-meta">retries: {{ job.retry_count }} · {{ job.created_at | date:'HH:mm:ss' }}</div>
          }
        </div>
      }
    }
    </div>
    }
    `,
  styles: [`
    .erno-devtools {
      position: fixed;
      bottom: 16px;
      right: 16px;
      background: rgba(0,0,0,0.88);
      color: #fff;
      padding: 12px 16px;
      border-radius: 8px;
      font-size: 12px;
      font-family: monospace;
      z-index: 9999;
      display: flex;
      flex-direction: column;
      gap: 6px;
      width: 220px;
      max-height: 480px;
      overflow-y: auto;
    }
    .erno-devtools.wide { width: 380px; }
    .header { display: flex; flex-direction: column; gap: 4px; }
    .tabs { display: flex; gap: 4px; margin-top: 4px; }
    .tabs button {
      background: #333; color: #ccc; border: none; border-radius: 4px;
      padding: 2px 8px; cursor: pointer; font-size: 11px; font-family: monospace;
    }
    .tabs button.active { background: #555; color: #fff; }
    .email-toolbar { display: flex; gap: 4px; justify-content: flex-end; }
    .empty { color: #888; font-style: italic; }
    .email-row { border-top: 1px solid #333; padding-top: 4px; }
    .email-summary { cursor: pointer; display: flex; gap: 4px; align-items: baseline; }
    .arrow { flex-shrink: 0; }
    .email-card {
      display: flex; flex-direction: column; gap: 2px; cursor: pointer;
      padding: 6px 8px; border-radius: 6px; background: #1c1c1c;
      border: 1px solid #333;
    }
    .email-card:hover { background: #262626; border-color: #4a4a4a; }
    .email-line { display: flex; gap: 8px; align-items: baseline; justify-content: space-between; }
    .email-card .subject, .email-card .to { flex: 1; min-width: 0; }
    .subject { font-weight: bold; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .time { color: #777; font-size: 10px; flex-shrink: 0; }
    .to { color: #aaa; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .actions { display: flex; gap: 6px; align-items: center; flex-shrink: 0; }
    .open-hint { color: #666; font-size: 10px; }
    .email-card:hover .open-hint { color: #6af; }
    .icon-btn {
      background: none; color: #777; border: none; padding: 0 2px;
      cursor: pointer; font-size: 13px; line-height: 1; font-family: monospace;
    }
    .icon-btn:hover { color: #f66; }
    .body-text { white-space: pre-wrap; font-size: 11px; color: #ccc; max-height: 200px; overflow-y: auto; margin: 4px 0; }
    .job-meta { color: #888; font-size: 10px; margin-top: 2px; }
    .status-pending { color: #fa0; }
    .status-pending_retry { color: #f80; }
    .status-running { color: #4af; }
    .status-completed { color: #4f4; }
    .status-failed { color: #f44; }
    button { cursor: pointer; }
  `],
})
export class ErnoDevtoolsComponent implements OnInit {
  readonly visible = isDevMode();
  readonly syncStatus$;
  wsStatus = 'disconnected';

  tab: Tab = 'status';
  emails = signal<MockEmail[]>([]);
  jobs = signal<DevJob[]>([]);
  expandedJob = signal<string | null>(null);

  constructor(
    private sync: ErnoSyncService,
    private mailService: ErnoDevMailService,
    private jobsService: ErnoDevJobsService,
  ) {
    this.syncStatus$ = this.sync.status$;
  }

  ngOnInit(): void {
    // WS connection state will be surfaced via ErnoRealtimeService in a later iteration
  }

  forceSync(): void {
    this.sync.pullDelta();
  }

  switchToEmails(): void {
    this.tab = 'emails';
    this.loadEmails();
  }

  /** Emails render their own CSS best in a real document, so open a tab. */
  openEmail(email: MockEmail): void {
    window.open(this.mailService.previewUrl(email.id), '_blank', 'noopener');
  }

  loadEmails(): void {
    this.mailService.list().subscribe(emails => {
      this.emails.set([...emails].reverse());
    });
  }

  deleteEmail(id: string): void {
    this.mailService.delete(id).subscribe(() => {
      this.emails.update(list => list.filter(e => e.id !== id));
    });
  }

  clearAll(): void {
    this.mailService.clear().subscribe(() => {
      this.emails.set([]);
    });
  }

  switchToJobs(): void {
    this.tab = 'jobs';
    this.loadJobs();
  }

  toggleJob(id: string): void {
    this.expandedJob.set(this.expandedJob() === id ? null : id);
  }

  loadJobs(): void {
    this.jobsService.list().subscribe(jobs => {
      this.jobs.set(jobs);
    });
  }

  clearJobs(): void {
    this.jobsService.clear().subscribe(() => {
      this.jobs.set([]);
      this.expandedJob.set(null);
    });
  }
}
