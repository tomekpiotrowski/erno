import { Component, inject, signal } from '@angular/core';
import { AdminApi, Dashboard } from '../core/api';

@Component({
  selector: 'app-dashboard',
  template: `
    <h1>Dashboard</h1>
    @if (error()) {
      <p class="error">{{ error() }}</p>
    }
    @if (data(); as d) {
      <div class="grid cards">
        <div class="card">Users<strong>{{ d.total_users }}</strong></div>
        <div class="card">Stripe<strong>{{ d.stripe_active }}</strong></div>
        <div class="card">Trial<strong>{{ d.trial_active }}</strong></div>
        <div class="card">Gift<strong>{{ d.gift_active }}</strong></div>
        <div class="card">No sub<strong>{{ d.no_sub }}</strong></div>
        <div class="card">Jobs pending<strong>{{ d.pending_jobs }}</strong></div>
        <div class="card">Jobs failed<strong>{{ d.failed_jobs }}</strong></div>
        <div class="card">Avg job ms<strong>{{ d.avg_execution_ms }}</strong></div>
      </div>
      <h2>Email outbox</h2>
      <table>
        <tr><th>Template</th><th>Total</th><th>Sent</th><th>Failed</th></tr>
        @for (e of d.email_stats; track e.name) {
          <tr>
            <td>{{ e.name }}</td>
            <td>{{ e.total }}</td>
            <td>{{ e.completed }}</td>
            <td>{{ e.failed }}</td>
          </tr>
        }
      </table>
      <p class="muted">Refreshed {{ d.refreshed_at }}</p>
    }
  `,
})
export class DashboardPage {
  private readonly api = inject(AdminApi);
  data = signal<Dashboard | null>(null);
  error = signal('');

  constructor() {
    this.api.dashboard().subscribe({
      next: (d) => this.data.set(d),
      error: () => this.error.set('Could not load dashboard'),
    });
  }
}
