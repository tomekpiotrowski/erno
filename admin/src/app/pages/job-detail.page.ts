import { JsonPipe } from '@angular/common';
import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { AdminApi, JobDetail } from '../core/api';

@Component({
  selector: 'app-job-detail',
  imports: [JsonPipe],
  template: `
    @if (data(); as d) {
      <h1>{{ d.job.job_type }}</h1>
      <p>{{ d.job.status }} · retries {{ d.job.retry_count }}</p>
      <button (click)="retry()">Retry</button>
      <h2>Arguments</h2>
      <pre>{{ d.arguments | json }}</pre>
      <h2>Executions</h2>
      <table>
        <tr><th>Result</th><th>ms</th><th>Reason</th><th>Finished</th></tr>
        @for (e of d.executions; track e.id) {
          <tr>
            <td>{{ e.result }}</td>
            <td>{{ e.execution_time_ms }}</td>
            <td>{{ e.failure_reason || '—' }}</td>
            <td>{{ e.finished_at }}</td>
          </tr>
        }
      </table>
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
}
