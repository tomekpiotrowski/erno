import { Component, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { AdminApi, TablesResponse } from '../core/api';
import { PrometheusService, PromPoint } from '../core/prometheus';
import { BUSINESS } from '../metrics/catalog';
import { Sparkline } from '../sparkline';

@Component({
  selector: 'app-database',
  imports: [Sparkline],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Database</h1>
          <p class="sub">Approximate live tuples from pg_stat_user_tables (not COUNT(*)).</p>
        </div>
      </header>

      @if (error()) {
        <p class="error">{{ error() }}</p>
      }

      @if (data(); as d) {
        <section class="panel flush">
          <table>
            <thead>
              <tr>
                <th>Table</th>
                <th class="num">≈ rows</th>
                <th class="num">Dead</th>
                <th>Last analyze</th>
                <th>History</th>
              </tr>
            </thead>
            <tbody>
              @for (t of d.tables; track t.table) {
                <tr>
                  <td class="mono">{{ t.table }}</td>
                  <td class="num">{{ t.approx_rows }}</td>
                  <td class="num">{{ t.n_dead_tup }}</td>
                  <td class="id">{{ t.last_analyze || '—' }}</td>
                  <td><app-sparkline [points]="history()[t.table] || []" /></td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      }
    </div>
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
