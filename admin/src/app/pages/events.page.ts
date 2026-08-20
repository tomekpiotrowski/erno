import { JsonPipe } from '@angular/common';
import { Component, inject, signal, ChangeDetectionStrategy } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AdminApi, EventsResponse } from '../core/api';

@Component({
  selector: 'app-events',
  imports: [FormsModule, JsonPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Audit log</h1>
          <p class="sub">Admin and system events.</p>
        </div>
      </header>

      <div class="toolbar">
        <input [ngModel]="name()" (ngModelChange)="name.set($event)" placeholder="name (user.registered)" />
        <button type="button" (click)="load()">Filter</button>
      </div>

      @if (data(); as d) {
        <section class="panel flush">
          <table>
            <thead>
              <tr><th>When</th><th>Name</th><th>User</th><th>Payload</th></tr>
            </thead>
            <tbody>
              @for (e of d.events; track e.id) {
                <tr>
                  <td class="id">{{ e.created_at }}</td>
                  <td class="mono">{{ e.name }}</td>
                  <td class="id">{{ e.user_id || '—' }}</td>
                  <td><code>{{ e.payload | json }}</code></td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      }
    </div>
  `,
})
export class EventsPage {
  private readonly api = inject(AdminApi);
  name = signal('');
  data = signal<EventsResponse | null>(null);
  constructor() {
    this.load();
  }
  load() {
    this.api.events(this.name()).subscribe((d) => this.data.set(d));
  }
}
