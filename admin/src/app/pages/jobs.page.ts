import { Component, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { AdminApi, JobsResponse } from '../core/api';

@Component({
  selector: 'app-jobs',
  imports: [RouterLink],
  template: `
    <h1>Jobs</h1>
    @if (data(); as d) {
      <table>
        <tr><th>Type</th><th>Pending</th><th>Running</th><th>Failed</th><th>Done</th></tr>
        @for (s of d.stats; track s.job_type) {
          <tr>
            <td>{{ s.job_type }}</td>
            <td>{{ s.pending }}</td>
            <td>{{ s.running }}</td>
            <td>{{ s.failed }}</td>
            <td>{{ s.completed }}</td>
          </tr>
        }
      </table>
      <h2>Recent</h2>
      <table>
        <tr><th>Type</th><th>Status</th><th>Retries</th><th>Created</th></tr>
        @for (j of d.jobs; track j.id) {
          <tr>
            <td><a [routerLink]="['/jobs', j.id]">{{ j.job_type }}</a></td>
            <td>{{ j.status }}</td>
            <td>{{ j.retry_count }}</td>
            <td>{{ j.created_at }}</td>
          </tr>
        }
      </table>
    }
  `,
})
export class JobsPage {
  private readonly api = inject(AdminApi);
  data = signal<JobsResponse | null>(null);
  constructor() {
    this.api.jobs().subscribe((d) => this.data.set(d));
  }
}
