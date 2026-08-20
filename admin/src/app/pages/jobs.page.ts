import { Component, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { RouterLink } from '@angular/router';
import { AdminApi, JobsResponse } from '../core/api';

@Component({
  selector: 'app-jobs',
  imports: [RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Jobs</h1>
          <p class="sub">Queue health and recent work.</p>
        </div>
      </header>

      @if (data(); as d) {
        @if (failed(d) > 0) {
          <div class="banner">
            <strong class="error">{{ failed(d) }} job types have failures.</strong>
            <span>Retry from the job detail.</span>
          </div>
        }

        <section class="panel flush">
          <header class="phead"><span class="eyebrow">By type</span></header>
          <table>
            <thead>
              <tr>
                <th>Type</th>
                <th class="num">Pending</th>
                <th class="num">Running</th>
                <th class="num">Failed</th>
                <th class="num">Done</th>
              </tr>
            </thead>
            <tbody>
              @for (s of d.stats; track s.job_type) {
                <tr [class.flag]="s.failed > 0">
                  <td>{{ s.job_type }}</td>
                  <td class="num">{{ s.pending }}</td>
                  <td class="num">{{ s.running }}</td>
                  <td class="num">{{ s.failed }}</td>
                  <td class="num">{{ s.completed }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>

        <section class="panel flush">
          <header class="phead"><span class="eyebrow">Recent</span></header>
          <table>
            <thead>
              <tr><th>Type</th><th>Status</th><th class="num">Retries</th><th>Created</th></tr>
            </thead>
            <tbody>
              @for (j of d.jobs; track j.id) {
                <tr>
                  <td><a [routerLink]="['/jobs', j.id]">{{ j.job_type }}</a></td>
                  <td><span class="status" [class]="statusClass(j.status)">{{ j.status }}</span></td>
                  <td class="num">{{ j.retry_count }}</td>
                  <td class="id">{{ j.created_at }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      }
    </div>
  `,
})
export class JobsPage {
  private readonly api = inject(AdminApi);
  data = signal<JobsResponse | null>(null);
  constructor() {
    this.api.jobs().subscribe((d) => this.data.set(d));
  }

  failed(d: JobsResponse) {
    return d.stats.reduce((n, s) => n + (s.failed > 0 ? 1 : 0), 0);
  }

  statusClass(status: string) {
    return `status ${this.tone(status)}`;
  }

  tone(status: string) {
    switch (status) {
      case 'completed':
      case 'ok':
        return 'good';
      case 'failed':
        return 'bad';
      case 'running':
        return 'info';
      case 'pending':
        return 'warn';
      default:
        return '';
    }
  }
}
