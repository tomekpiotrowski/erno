import { JsonPipe } from '@angular/common';
import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';
import { AdminApi, JobDetail } from '../core/api';

@Component({
  selector: 'app-job-detail',
  imports: [JsonPipe, RouterLink],
  template: `
    @if (data(); as d) {
      <div class="stack">
        <header class="head">
          <div>
            <a routerLink="/jobs" class="muted">← Jobs</a>
            <h1>{{ d.job.job_type }}</h1>
            <p class="sub">
              <span [class]="statusClass(d.job.status)">{{ d.job.status }}</span>
              · retries {{ d.job.retry_count }}
            </p>
          </div>
          <button type="button" (click)="retry()">Retry</button>
        </header>

        <section class="panel">
          <header class="phead"><span class="eyebrow">Arguments</span></header>
          <pre>{{ d.arguments | json }}</pre>
        </section>

        <section class="panel flush">
          <header class="phead"><span class="eyebrow">Executions</span></header>
          <table>
            <thead>
              <tr><th>Result</th><th class="num">ms</th><th>Reason</th><th>Finished</th></tr>
            </thead>
            <tbody>
              @for (e of d.executions; track e.id) {
                <tr>
                  <td><span [class]="statusClass(e.result)">{{ e.result }}</span></td>
                  <td class="num">{{ e.execution_time_ms }}</td>
                  <td class="id">{{ e.failure_reason || '—' }}</td>
                  <td class="id">{{ e.finished_at }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      </div>
    }
  `,
})
export class JobDetailPage {
  private readonly api = inject(AdminApi);
  private readonly route = inject(ActivatedRoute);
  data = signal<JobDetail | null>(null);

  constructor() {
    this.reload();
  }

  reload() {
    const id = this.route.snapshot.paramMap.get('id')!;
    this.api.job(id).subscribe((d) => this.data.set(d));
  }

  retry() {
    const id = this.route.snapshot.paramMap.get('id')!;
    this.api.retry(id).subscribe(() => this.reload());
  }

  statusClass(status: string) {
    return `status ${this.tone(status)}`;
  }

  tone(status: string) {
    switch (status) {
      case 'completed':
      case 'ok':
      case 'success':
        return 'good';
      case 'failed':
      case 'error':
        return 'bad';
      case 'running':
        return 'info';
      default:
        return 'warn';
    }
  }
}
