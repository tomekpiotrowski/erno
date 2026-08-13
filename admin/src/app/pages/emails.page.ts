import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AdminApi, EmailList } from '../core/api';

@Component({
  selector: 'app-emails',
  imports: [FormsModule],
  template: `
    <h1>Emails</h1>
    <div class="toolbar">
      <input [(ngModel)]="to" placeholder="Filter to" (keyup.enter)="load()" />
      <button (click)="load()">Search</button>
    </div>
    @if (data(); as d) {
      <table>
        <tr><th>To</th><th>Subject</th><th>Template</th><th>Status</th><th>Sent</th></tr>
        @for (e of d.emails; track e.id) {
          <tr>
            <td>{{ e.to }}</td>
            <td>{{ e.subject }}</td>
            <td>{{ e.template || '—' }}</td>
            <td>{{ e.status }}</td>
            <td>{{ e.sent_at || e.created_at }}</td>
          </tr>
        }
      </table>
      <p class="muted">{{ d.total }} messages</p>
    }
  `,
})
export class EmailsPage {
  private readonly api = inject(AdminApi);
  to = '';
  data = signal<EmailList | null>(null);
  constructor() {
    this.load();
  }
  load() {
    this.api.emails(this.to).subscribe((d) => this.data.set(d));
  }
}
