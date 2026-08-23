import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { CollectorApi, ReleaseList } from '../core/api';

// Docs: docs/src/content/docs/monitoring/releases.md

@Component({
  selector: 'app-releases',
  imports: [RouterLink],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Releases</h1>
          <p class="sub">
            Deploys recorded by CI, newest first. "New issues" counts error types
            whose first sighting carried that version.
          </p>
        </div>
      </header>

      @if (data(); as d) {
        @if (!d.releases.length) {
          <section class="panel">
            <p class="muted">
              No deploys recorded yet. Have CI post to
              <code>POST /api/collector/releases</code> with the server ingest
              token.
            </p>
          </section>
        } @else {
          <section class="panel flush">
            <table>
              <thead>
                <tr>
                  <th>Deployed</th>
                  <th>Version</th>
                  <th>Environment</th>
                  <th>Commit</th>
                  <th>Source</th>
                  <th class="num">New issues</th>
                </tr>
              </thead>
              <tbody>
                @for (r of d.releases; track r.id) {
                  <tr [class.flag]="r.new_issues > 0">
                    <td class="id">{{ r.deployed_at }}</td>
                    <td class="mono">{{ r.version }}</td>
                    <td>{{ r.environment }}</td>
                    <td class="mono">{{ r.commit_sha ?? '—' }}</td>
                    <td class="muted">{{ r.source ?? '—' }}</td>
                    <td class="num">
                      @if (r.new_issues > 0) {
                        <a [routerLink]="['/issues']" [queryParams]="{ release: r.version }">
                          {{ r.new_issues }}
                        </a>
                      } @else {
                        0
                      }
                    </td>
                  </tr>
                }
              </tbody>
            </table>
          </section>
        }
      }
    </div>
  `,
})
export class ReleasesPage {
  private readonly api = inject(CollectorApi);
  data = signal<ReleaseList | null>(null);

  constructor() {
    this.api.releases().subscribe((d) => this.data.set(d));
  }
}
