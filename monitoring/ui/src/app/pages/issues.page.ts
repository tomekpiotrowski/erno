import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { ActivatedRoute, RouterLink } from '@angular/router';
import {
  CollectorApi,
  IssueCounts,
  IssueList,
  IssueStatus,
  PromPoint,
  toPromPoints,
} from '../core/api';
import { Sparkline } from '../sparkline';

// Docs: docs/src/content/docs/monitoring/error-reporting.md

const WINDOWS = [
  { id: '1h', hours: 1 },
  { id: '24h', hours: 24 },
  { id: '7d', hours: 168 },
  { id: '30d', hours: 720 },
  { id: '90d', hours: 2160 },
] as const;

const SOURCES = ['all', 'api', 'app', 'admin'] as const;
const STATUSES: IssueStatus[] = ['unresolved', 'resolved', 'ignored', 'all'];

@Component({
  selector: 'app-issues',
  imports: [FormsModule, RouterLink, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Issues</h1>
          <p class="sub">Grouped by fingerprint. Most recently seen first.</p>
        </div>
        @if (series().length) {
          <app-sparkline [points]="series()" />
        }
      </header>

      @if (counts(); as c) {
        <div class="grid cards">
          <div class="card stat" [class.alert]="c.unresolved > 0">
            <span class="label">Unresolved</span>
            <span class="value">{{ c.unresolved }}</span>
          </div>
          <div class="card stat">
            <span class="label">Resolved</span>
            <span class="value">{{ c.resolved }}</span>
          </div>
          <div class="card stat">
            <span class="label">Ignored</span>
            <span class="value">{{ c.ignored }}</span>
          </div>
        </div>
      }

      <div class="toolbar">
        @for (s of statuses; track s) {
          <button type="button" class="filter" [class.on]="status() === s" (click)="setStatus(s)">
            {{ s }}
          </button>
        }
      </div>

      <div class="toolbar">
        @for (s of sources; track s) {
          <button type="button" class="filter" [class.on]="source() === s" (click)="setSource(s)">
            {{ s }}
          </button>
        }
        @for (w of windows; track w.id) {
          <button
            type="button"
            class="chip"
            [class.on]="hours() === w.hours"
            (click)="setHours(w.hours)"
          >
            {{ w.id }}
          </button>
        }
        <input
          [ngModel]="q()"
          (ngModelChange)="q.set($event)"
          (keyup.enter)="search()"
          placeholder="Search title or type"
        />
        <button type="button" (click)="search()">Search</button>
        @if (release()) {
          <button type="button" class="chip on" (click)="clearRelease()">
            release {{ release() }} ✕
          </button>
        }
      </div>

      @if (data(); as d) {
        @if (!d.issues.length) {
          <section class="panel">
            <p class="muted">Nothing here — no issues match these filters.</p>
          </section>
        } @else {
          <section class="panel flush">
            <table>
              <thead>
                <tr>
                  <th>Last seen</th>
                  <th>Source</th>
                  <th>Type</th>
                  <th>Title</th>
                  <th>Culprit</th>
                  <th class="num">Events</th>
                  <th>Release</th>
                  <th>Status</th>
                </tr>
              </thead>
              <tbody>
                @for (i of d.issues; track i.id) {
                  <tr [class.flag]="i.status === 'unresolved'">
                    <td class="id">{{ i.last_seen }}</td>
                    <td><span class="status info">{{ i.source }}</span></td>
                    <td class="mono">{{ i.error_type }}</td>
                    <td><a [routerLink]="['/issues', i.id]">{{ i.title }}</a></td>
                    <td class="id" [title]="i.culprit ?? ''">{{ i.culprit ?? '—' }}</td>
                    <td class="num">{{ i.times_seen }}</td>
                    <td class="mono">{{ i.last_release ?? '—' }}</td>
                    <td><span [class]="statusClass(i.status)">{{ i.status }}</span></td>
                  </tr>
                }
              </tbody>
            </table>
          </section>

          <div class="toolbar">
            <span class="muted">{{ d.total }} issues · page {{ d.page }}</span>
            <button type="button" [disabled]="d.page <= 1" (click)="goto(d.page - 1)">Prev</button>
            <button
              type="button"
              [disabled]="d.page * d.per_page >= d.total"
              (click)="goto(d.page + 1)"
            >
              Next
            </button>
          </div>
        }
      }
    </div>
  `,
})
export class IssuesPage {
  private readonly api = inject(CollectorApi);

  readonly windows = WINDOWS;
  readonly sources = SOURCES;
  readonly statuses = STATUSES;

  status = signal<IssueStatus>('unresolved');
  source = signal<string>('all');
  hours = signal(168);
  q = signal('');
  page = signal(1);
  /** Set when arriving from the releases page. */
  release = signal('');

  data = signal<IssueList | null>(null);
  counts = signal<IssueCounts | null>(null);
  series = signal<PromPoint[]>([]);

  private readonly route = inject(ActivatedRoute);

  constructor() {
    const release = this.route.snapshot.queryParamMap.get('release');
    if (release) {
      this.release.set(release);
      // Arriving from a release means "show me what this deploy did", which is
      // not limited to what is still unresolved.
      this.status.set('all');
    }
    this.load();
  }

  load() {
    this.api
      .issues(
        this.status(),
        this.source(),
        this.q(),
        this.hours(),
        this.page(),
        50,
        this.release(),
      )
      .subscribe((d) => this.data.set(d));
    this.api.counts(this.hours()).subscribe((c) => this.counts.set(c));
    this.api
      .series(Math.min(this.hours(), 168), this.source())
      .subscribe((s) => this.series.set(toPromPoints(s)));
  }

  /** Any filter change invalidates the current page number. */
  private reset() {
    this.page.set(1);
    this.load();
  }

  setStatus(status: IssueStatus) {
    this.status.set(status);
    this.reset();
  }

  setSource(source: string) {
    this.source.set(source);
    this.reset();
  }

  setHours(hours: number) {
    this.hours.set(hours);
    this.reset();
  }

  search() {
    this.reset();
  }

  clearRelease() {
    this.release.set('');
    this.reset();
  }

  goto(page: number) {
    this.page.set(page);
    this.load();
  }

  statusClass(status: string) {
    switch (status) {
      case 'resolved':
        return 'status good';
      case 'ignored':
        return 'status warn';
      default:
        return 'status bad';
    }
  }
}
