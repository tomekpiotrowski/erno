import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { JsonPipe } from '@angular/common';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import {
  CollectorApi,
  ErrorEvent,
  IssueDetail,
  PromPoint,
  toPromPoints,
} from '../core/api';
import { Sparkline } from '../sparkline';

// Docs: docs/src/content/docs/monitoring/error-reporting.md

@Component({
  selector: 'app-issue-detail',
  imports: [RouterLink, JsonPipe, Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (data(); as d) {
      <div class="stack">
        <header class="head">
          <div>
            <a routerLink="/issues" class="muted">← Issues</a>
            <h1>{{ d.issue.error_type }}</h1>
            <p class="sub">
              <span [class]="statusClass(d.issue.status)">{{ d.issue.status }}</span>
              · {{ d.issue.title }}
            </p>
          </div>
          <div class="actions">
            @if (d.issue.status !== 'resolved') {
              <button type="button" (click)="resolve()">Resolve</button>
            }
            @if (d.issue.status !== 'ignored') {
              <button type="button" (click)="ignore()">Ignore</button>
            }
            @if (d.issue.status !== 'unresolved') {
              <button type="button" (click)="unresolve()">Unresolve</button>
            }
            <button type="button" class="ghost" (click)="confirming.set(true)">Delete</button>
          </div>
        </header>

        @if (series().length) {
          <section class="panel">
            <header class="phead"><span class="eyebrow">Last 24 hours</span></header>
            <app-sparkline [points]="series()" />
          </section>
        }

        <section class="panel">
          <header class="phead"><span class="eyebrow">Details</span></header>
          <dl class="kv">
            <dt>Fingerprint</dt>
            <dd class="mono">{{ d.issue.fingerprint }}</dd>
            <dt>Source</dt>
            <dd>{{ d.issue.source }}</dd>
            <dt>Level</dt>
            <dd>{{ d.issue.level }}</dd>
            <dt>Culprit</dt>
            <dd class="mono">{{ d.issue.culprit ?? '—' }}</dd>
            <dt>Occurrences</dt>
            <!-- Deliberately two numbers: the burst cap bounds stored rows
                 while every occurrence is still counted. -->
            <dd class="mono">
              {{ d.issue.times_seen }} counted · {{ d.stored_events }} stored
            </dd>
            <dt>First seen</dt>
            <dd class="mono">{{ d.issue.first_seen }} · {{ d.issue.first_release ?? '—' }}</dd>
            <dt>Last seen</dt>
            <dd class="mono">{{ d.issue.last_seen }} · {{ d.issue.last_release ?? '—' }}</dd>
            <dt>Environment</dt>
            <dd>{{ d.issue.environment ?? '—' }}</dd>
          </dl>
        </section>

        @if (selected(); as ev) {
          <section class="panel">
            <header class="phead">
              <span class="eyebrow">Stack</span>
              <span class="muted">{{ ev.created_at }}</span>
            </header>
            @if (ev.stack) {
              <pre>{{ ev.stack }}</pre>
            } @else if (ev.frames?.length) {
              <table>
                <thead>
                  <tr><th>Function</th><th>File</th><th class="num">Line</th></tr>
                </thead>
                <tbody>
                  @for (f of ev.frames; track $index) {
                    <tr [class.muted]="!f.in_app">
                      <td class="mono">{{ f.function ?? '—' }}</td>
                      <td class="id">{{ f.file ?? '—' }}</td>
                      <td class="num">{{ f.line ?? '—' }}</td>
                    </tr>
                  }
                </tbody>
              </table>
            } @else {
              <p class="muted">
                No stack was captured. Grouping fell back to the call site and a
                normalised message.
              </p>
            }
          </section>

          <section class="panel">
            <header class="phead"><span class="eyebrow">Context</span></header>
            <pre>{{ ev.context | json }}</pre>
            @if (ev.user_id) {
              <p class="muted">Affected user: {{ ev.user_email ?? ev.user_id }}</p>
            }
          </section>
        }

        <section class="panel flush">
          <header class="phead"><span class="eyebrow">Occurrences</span></header>
          <table>
            <thead>
              <tr><th>When</th><th>Release</th><th>Environment</th><th>User</th></tr>
            </thead>
            <tbody>
              @for (ev of d.events; track ev.id) {
                <tr (click)="selected.set(ev)" class="clickable">
                  <td class="id">{{ ev.created_at }}</td>
                  <td class="mono">{{ ev.release ?? '—' }}</td>
                  <td>{{ ev.environment ?? '—' }}</td>
                  <td>{{ ev.user_email ?? ev.user_id ?? '—' }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      </div>

      @if (confirming()) {
        <div class="overlay">
          <div class="dialog">
            <h2>Delete this issue?</h2>
            <p class="muted">
              Removes the issue and all {{ d.stored_events }} stored events. This cannot
              be undone; if the error happens again it will come back as a new issue.
            </p>
            <div class="actions">
              <button type="button" class="ghost" (click)="confirming.set(false)">Cancel</button>
              <button type="button" (click)="remove()">Delete</button>
            </div>
          </div>
        </div>
      }
    }
  `,
  styles: `
    .clickable { cursor: pointer; }
  `,
})
export class IssueDetailPage {
  private readonly api = inject(CollectorApi);
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);

  data = signal<IssueDetail | null>(null);
  selected = signal<ErrorEvent | null>(null);
  series = signal<PromPoint[]>([]);
  confirming = signal(false);

  constructor() {
    this.reload();
  }

  private id(): string {
    return this.route.snapshot.paramMap.get('id') ?? '';
  }

  reload() {
    this.api.issue(this.id()).subscribe((d) => {
      this.data.set(d);
      this.selected.set(d.latest_event);
    });
    this.api.issueSeries(this.id(), 24).subscribe((s) => this.series.set(toPromPoints(s)));
  }

  resolve() {
    this.api.resolve(this.id()).subscribe(() => this.reload());
  }

  ignore() {
    this.api.ignore(this.id()).subscribe(() => this.reload());
  }

  unresolve() {
    this.api.unresolve(this.id()).subscribe(() => this.reload());
  }

  remove() {
    this.api.remove(this.id()).subscribe(() => {
      this.confirming.set(false);
      void this.router.navigate(['/issues']);
    });
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
