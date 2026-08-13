import { JsonPipe } from '@angular/common';
import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AdminApi, EventsResponse } from '../core/api';

@Component({
  selector: 'app-events',
  imports: [FormsModule, JsonPipe],
  template: `
    <h1>Events</h1>
    <div class="toolbar">
      <input [(ngModel)]="name" placeholder="name (user.registered)" />
      <button (click)="load()">Filter</button>
    </div>
    @if (data(); as d) {
      <table>
        <tr><th>When</th><th>Name</th><th>User</th><th>Payload</th></tr>
        @for (e of d.events; track e.id) {
          <tr>
            <td>{{ e.created_at }}</td>
            <td>{{ e.name }}</td>
            <td>{{ e.user_id || '—' }}</td>
            <td><code>{{ e.payload | json }}</code></td>
          </tr>
        }
      </table>
    }
  `,
})
export class EventsPage {
  private readonly api = inject(AdminApi);
  name = '';
  data = signal<EventsResponse | null>(null);
  constructor() {
    this.load();
  }
  load() {
    this.api.events(this.name).subscribe((d) => this.data.set(d));
  }
}
