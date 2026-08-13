import { Component, inject, signal } from '@angular/core';
import { AdminApi, TablesResponse } from '../core/api';
import { PrometheusService, PromPoint } from '../core/prometheus';
import { BUSINESS } from '../metrics/catalog';
import { Sparkline } from '../sparkline';

@Component({
  selector: 'app-database',
  imports: [Sparkline],
  template: `
    <h1>Database</h1>
    <p class="muted">Approximate live tuples from pg_stat_user_tables (not COUNT(*)).</p>
    @if (error()) {
      <p class="error">{{ error() }}</p>
    }
    @if (data(); as d) {
      <table>
        <tr><th>Table</th><th>≈ rows</th><th>Dead</th><th>Last analyze</th><th>History</th></tr>
        @for (t of d.tables; track t.table) {
          <tr>
            <td>{{ t.table }}</td>
            <td>{{ t.approx_rows }}</td>
            <td>{{ t.n_dead_tup }}</td>
            <td>{{ t.last_analyze || '—' }}</td>
            <td><app-sparkline [points]="history()[t.table] || []" /></td>
          </tr>
        }
      </table>
    }
  `,
})
export class DatabasePage {
  private readonly api = inject(AdminApi);
  private readonly prom = inject(PrometheusService);
  data = signal<TablesResponse | null>(null);
  history = signal<Record<string, PromPoint[]>>({});
  error = signal('');
  constructor() {
    this.api.tables().subscribe({
      next: (d) => this.data.set(d),
      error: () => this.error.set('Could not load table stats'),
    });
    this.prom.range(BUSINESS.tableCount, 86400, '5m').subscribe({
      next: (series) => {
        const map: Record<string, PromPoint[]> = {};
        for (const s of series) {
          const table = s.metric['table'];
          if (table) map[table] = s.points;
        }
        this.history.set(map);
      },
      error: () => {
        /* Prometheus optional on this page */
      },
    });
  }
}
