import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { AdminApi, EmailList } from '../core/api';

@Component({
  selector: 'app-emails',
  imports: [FormsModule],
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Emails</h1>
          <p class="sub">Transactional outbox.</p>
        </div>
      </header>

      <div class="toolbar">
        <input [(ngModel)]="to" placeholder="Filter to" (keyup.enter)="load()" />
        <button type="button" (click)="load()">Search</button>
      </div>

      @if (data(); as d) {
        <section class="panel flush">
          <table>
            <thead>
              <tr><th>To</th><th>Subject</th><th>Template</th><th>Status</th><th>Sent</th></tr>
            </thead>
            <tbody>
              @for (e of d.emails; track e.id) {
                <tr>
                  <td>{{ e.to }}</td>
                  <td>{{ e.subject }}</td>
                  <td class="id">{{ e.template || '—' }}</td>
                  <td><span [class]="statusClass(e.status)">{{ e.status }}</span></td>
                  <td class="id">{{ e.sent_at || e.created_at }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
        <p class="muted">{{ d.total }} messages</p>
      }
    </div>
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

  statusClass(status: string) {
    return `status ${this.tone(status)}`;
  }

  tone(status: string) {
    switch (status) {
      case 'sent':
      case 'completed':
        return 'good';
      case 'failed':
      case 'bounced':
        return 'bad';
      case 'pending':
        return 'warn';
      default:
        return 'info';
    }
  }
}
