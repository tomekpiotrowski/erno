import { ChangeDetectionStrategy, Component, isDevMode, OnInit, signal } from '@angular/core';
import { ErnoSyncService } from '../sync/erno-sync.service';
import { ErnoDevMailService, MockEmail } from './erno-dev-mail.service';
import { ErnoDevJobsService, DevJob } from './erno-dev-jobs.service';

type Tab = 'status' | 'emails' | 'jobs';

@Component({
  selector: 'erno-devtools',
  standalone: false,
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div *ngIf="visible" class="erno-devtools" [class.wide]="tab === 'emails' || tab === 'jobs'">
      <div class="header">
        <strong>Erno Devtools</strong>
        <div class="tabs">
          <button [class.active]="tab === 'status'" (click)="tab = 'status'">Status</button>
          <button [class.active]="tab === 'emails'" (click)="switchToEmails()">
            Emails<span *ngIf="emails().length"> ({{ emails().length }})</span>
          </button>
          <button [class.active]="tab === 'jobs'" (click)="switchToJobs()">
            Jobs<span *ngIf="jobs().length"> ({{ jobs().length }})</span>
          </button>
        </div>
      </div>

      <ng-container *ngIf="tab === 'status'">
        <div>WS: {{ wsStatus }}</div>
        <div>Sync: {{ syncStatus$ | async }}</div>
        <button (click)="forceSync()">Force re-sync</button>
      </ng-container>

      <ng-container *ngIf="tab === 'emails'">
        <div class="email-toolbar">
          <button (click)="loadEmails()">↺</button>
          <button (click)="clearAll()" [disabled]="emails().length === 0">Clear all</button>
        </div>
        <div *ngIf="emails().length === 0" class="empty">No emails sent.</div>
        <div *ngFor="let email of emails()" class="email-row">
          <div class="email-summary" (click)="toggle(email.id)">
            <span class="arrow">{{ expanded() === email.id ? '▾' : '▸' }}</span>
            <span class="subject">{{ email.subject }}</span>
            <span class="to">→ {{ email.to }}</span>
          </div>
          <ng-container *ngIf="expanded() === email.id">
            <iframe *ngIf="email.body_html" [srcdoc]="email.body_html" sandbox class="body-frame"></iframe>
            <pre *ngIf="!email.body_html && email.body_text" class="body-text">{{ email.body_text }}</pre>
            <button class="delete-btn" (click)="deleteEmail(email.id)">× delete</button>
          </ng-container>
        </div>
      </ng-container>

      <ng-container *ngIf="tab === 'jobs'">
        <div class="email-toolbar">
          <button (click)="loadJobs()">↺</button>
          <button (click)="clearJobs()" [disabled]="jobs().length === 0">Clear all</button>
        </div>
        <div *ngIf="jobs().length === 0" class="empty">No jobs.</div>
        <div *ngFor="let job of jobs()" class="email-row">
          <div class="email-summary" (click)="toggleJob(job.id)">
            <span class="arrow">{{ expandedJob() === job.id ? '▾' : '▸' }}</span>
            <span class="subject">{{ job.type }}</span>
            <span class="to" [class]="'status-' + job.status">{{ job.status }}</span>
          </div>
          <ng-container *ngIf="expandedJob() === job.id">
            <pre class="body-text">{{ job.arguments | json }}</pre>
            <div class="job-meta">retries: {{ job.retry_count }} · {{ job.created_at | date:'HH:mm:ss' }}</div>
          </ng-container>
        </div>
      </ng-container>
    </div>
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
    .subject { font-weight: bold; flex-shrink: 0; max-width: 140px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .to { color: #aaa; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .body-frame { width: 100%; height: 200px; border: 1px solid #555; border-radius: 4px; margin-top: 4px; background: #fff; }
    .body-text { white-space: pre-wrap; font-size: 11px; color: #ccc; max-height: 200px; overflow-y: auto; margin: 4px 0; }
    .delete-btn { background: #600; color: #fff; border: none; border-radius: 4px; padding: 2px 6px; cursor: pointer; font-size: 11px; font-family: monospace; margin-top: 4px; }
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
  expanded = signal<string | null>(null);
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

  toggle(id: string): void {
    this.expanded.set(this.expanded() === id ? null : id);
  }

  loadEmails(): void {
    this.mailService.list().subscribe(emails => {
      this.emails.set(emails);
    });
  }

  deleteEmail(id: string): void {
    this.mailService.delete(id).subscribe(() => {
      this.emails.update(list => list.filter(e => e.id !== id));
      if (this.expanded() === id) this.expanded.set(null);
    });
  }

  clearAll(): void {
    this.mailService.clear().subscribe(() => {
      this.emails.set([]);
      this.expanded.set(null);
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
