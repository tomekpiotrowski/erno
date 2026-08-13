import { Component, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { AdminApi, Dashboard } from '../core/api';

@Component({
  selector: 'app-dashboard',
  imports: [RouterLink],
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Overview</h1>
          <p class="sub">What needs a person today.</p>
        </div>
        @if (data(); as d) {
          <span class="mono muted">refreshed {{ d.refreshed_at }}</span>
        }
      </header>

      @if (error()) {
        <p class="error">{{ error() }}</p>
      }

      @if (data(); as d) {
        @if (d.failed_jobs > 0) {
          <div class="banner">
            <strong class="error">{{ d.failed_jobs }} jobs failed.</strong>
            <span>Queue is held until they succeed.</span>
            <a routerLink="/jobs" class="btn">Open jobs</a>
          </div>
        }

        <div class="grid cards">
          <div class="stat">
            <span class="label">Users</span>
            <span class="value">{{ d.total_users }}</span>
          </div>
          <div class="stat">
            <span class="label">Stripe</span>
            <span class="value">{{ d.stripe_active }}</span>
          </div>
          <div class="stat">
            <span class="label">Trial</span>
            <span class="value">{{ d.trial_active }}</span>
          </div>
          <div class="stat">
            <span class="label">Gift</span>
            <span class="value">{{ d.gift_active }}</span>
          </div>
          <div class="stat">
            <span class="label">No sub</span>
            <span class="value">{{ d.no_sub }}</span>
          </div>
          <div class="stat">
            <span class="label">Jobs pending</span>
            <span class="value">{{ d.pending_jobs }}</span>
          </div>
          <div class="stat" [class.alert]="d.failed_jobs > 0">
            <span class="label">Jobs failed</span>
            <span class="value">{{ d.failed_jobs }}</span>
          </div>
          <div class="stat">
            <span class="label">Avg job ms</span>
            <span class="value">{{ d.avg_execution_ms }}</span>
          </div>
        </div>

        <section class="panel flush">
          <header class="phead"><span class="eyebrow">Email outbox</span></header>
          <table>
            <thead>
              <tr><th>Template</th><th class="num">Total</th><th class="num">Sent</th><th class="num">Failed</th></tr>
            </thead>
            <tbody>
              @for (e of d.email_stats; track e.name) {
                <tr>
                  <td>{{ e.name }}</td>
                  <td class="num">{{ e.total }}</td>
                  <td class="num">{{ e.completed }}</td>
                  <td class="num">{{ e.failed }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      }
    </div>
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
