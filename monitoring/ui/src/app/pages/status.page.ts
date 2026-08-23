import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CollectorApi, StatusComponent as Comp, StatusSnapshot, UptimeCheck } from '../core/api';

// Docs: docs/src/content/docs/monitoring/status-page.md

const STATES = ['operational', 'degraded', 'partial_outage', 'major_outage', 'maintenance'];

@Component({
  selector: 'app-status',
  imports: [FormsModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Status page</h1>
          <p class="sub">
            What the public sees. The page reads a published document rather than
            this service, so it keeps working during an outage.
          </p>
        </div>
        @if (snapshot(); as s) {
          <span [class]="pill(s.state)">{{ s.state.replace('_', ' ') }}</span>
        }
      </header>

      <section class="panel">
        <header class="phead"><span class="eyebrow">Add a component</span></header>
        <div class="toolbar">
          <input [(ngModel)]="name" placeholder="Name, e.g. API" />
          <input [(ngModel)]="description" placeholder="Description (optional)" />
          <select [(ngModel)]="checkId">
            <option value="">Operator-controlled</option>
            @for (c of checks(); track c.id) {
              <option [value]="c.id">Follow check: {{ c.name }}</option>
            }
          </select>
          <button type="button" (click)="addComponent()" [disabled]="!name()">Add</button>
        </div>
      </section>

      @if (components().length) {
        <section class="panel flush">
          <header class="phead"><span class="eyebrow">Components</span></header>
          <table>
            <thead>
              <tr><th>Name</th><th>Source</th><th>State</th><th></th></tr>
            </thead>
            <tbody>
              @for (c of components(); track c.id) {
                <tr>
                  <td>{{ c.name }}</td>
                  <td class="muted">
                    {{ c.auto_from_check_id ? 'follows an uptime check' : 'operator-controlled' }}
                  </td>
                  <td>
                    @if (c.auto_from_check_id) {
                      <span class="muted">from the probe</span>
                    } @else {
                      <select
                        [ngModel]="c.manual_state"
                        (ngModelChange)="setState(c.id, $event)"
                      >
                        @for (s of states; track s) {
                          <option [value]="s">{{ s.replace('_', ' ') }}</option>
                        }
                      </select>
                    }
                  </td>
                  <td>
                    <button type="button" class="ghost" (click)="removeComponent(c.id)">
                      Delete
                    </button>
                  </td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      }

      <section class="panel">
        <header class="phead"><span class="eyebrow">Open an incident</span></header>
        <div class="toolbar">
          <input [(ngModel)]="incidentTitle" placeholder="What is happening" />
          <select [(ngModel)]="incidentImpact">
            <option value="minor">minor</option>
            <option value="major">major</option>
            <option value="critical">critical</option>
          </select>
        </div>
        <div class="toolbar">
          <input [(ngModel)]="incidentBody" placeholder="First update, in plain language" />
          <button
            type="button"
            (click)="openIncident()"
            [disabled]="!incidentTitle() || !incidentBody()"
          >
            Publish
          </button>
        </div>
      </section>

      @if (snapshot(); as s) {
        @if (s.active_incidents.length) {
          <section class="panel">
            <header class="phead"><span class="eyebrow">Active incidents</span></header>
            @for (i of s.active_incidents; track i.id) {
              <div class="incident">
                <p>
                  <strong>{{ i.title }}</strong>
                  <span class="status bad">{{ i.impact }}</span>
                  <span class="status info">{{ i.status }}</span>
                </p>
                @for (u of i.updates; track u.created_at) {
                  <p class="muted"><span class="mono">{{ u.status }}</span> — {{ u.body }}</p>
                }
                <div class="toolbar">
                  <select [(ngModel)]="updateStatus">
                    <option value="investigating">investigating</option>
                    <option value="identified">identified</option>
                    <option value="monitoring">monitoring</option>
                    <option value="resolved">resolved</option>
                  </select>
                  <input [(ngModel)]="updateBody" placeholder="Add an update" />
                  <button type="button" (click)="addUpdate(i.id)" [disabled]="!updateBody()">
                    Post
                  </button>
                </div>
              </div>
            }
          </section>
        }
      }
    </div>
  `,
  styles: `
    .incident { padding: 0 13px 13px; }
    select { max-width: 220px; }
  `,
})
export class StatusPage {
  private readonly api = inject(CollectorApi);

  readonly states = STATES;

  snapshot = signal<StatusSnapshot | null>(null);
  components = signal<Comp[]>([]);
  checks = signal<UptimeCheck[]>([]);

  name = signal('');
  description = signal('');
  checkId = signal('');
  incidentTitle = signal('');
  incidentImpact = signal('minor');
  incidentBody = signal('');
  updateStatus = signal('identified');
  updateBody = signal('');

  constructor() {
    this.load();
  }

  load() {
    this.api.statusSnapshot().subscribe((s) => this.snapshot.set(s));
    this.api.statusComponents().subscribe((c) => this.components.set(c.components));
    this.api.uptime().subscribe((u) => this.checks.set(u.checks));
  }

  addComponent() {
    this.api
      .createStatusComponent({
        name: this.name(),
        description: this.description() || undefined,
        auto_from_check_id: this.checkId() || null,
      })
      .subscribe(() => {
        this.name.set('');
        this.description.set('');
        this.checkId.set('');
        this.load();
      });
  }

  removeComponent(id: string) {
    this.api.deleteStatusComponent(id).subscribe(() => this.load());
  }

  setState(id: string, state: string) {
    this.api.setStatusComponentState(id, state).subscribe(() => this.load());
  }

  openIncident() {
    this.api
      .openIncident({
        title: this.incidentTitle(),
        impact: this.incidentImpact(),
        body: this.incidentBody(),
      })
      .subscribe(() => {
        this.incidentTitle.set('');
        this.incidentBody.set('');
        this.load();
      });
  }

  addUpdate(id: string) {
    this.api
      .addIncidentUpdate(id, { status: this.updateStatus(), body: this.updateBody() })
      .subscribe(() => {
        this.updateBody.set('');
        this.load();
      });
  }

  pill(state: string): string {
    switch (state) {
      case 'operational':
        return 'status good';
      case 'degraded':
      case 'maintenance':
        return 'status warn';
      default:
        return 'status bad';
    }
  }
}
