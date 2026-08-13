import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { AdminApi, UserList } from '../core/api';

@Component({
  selector: 'app-users',
  imports: [FormsModule, RouterLink],
  template: `
    <h1>Users</h1>
    <div class="toolbar">
      <input [(ngModel)]="q" (keyup.enter)="load()" placeholder="Search email" />
      <button (click)="load()">Search</button>
    </div>
    @if (data(); as d) {
      <table>
        <tr>
          <th>Email</th>
          <th>Plan</th>
          <th>Verified</th>
          <th>Last active</th>
        </tr>
        @for (u of d.users; track u.id) {
          <tr>
            <td><a [routerLink]="['/users', u.id]">{{ u.email }}</a></td>
            <td>{{ u.subscription_plan || '—' }}</td>
            <td>{{ u.email_verified_at ? 'yes' : 'no' }}</td>
            <td>{{ u.last_active_at || '—' }}</td>
          </tr>
        }
      </table>
      <p class="muted">{{ d.total }} users · page {{ d.page }}</p>
      <div class="toolbar">
        <button [disabled]="d.page <= 1" (click)="page = d.page - 1; load()">Prev</button>
        <button [disabled]="d.page * d.per_page >= d.total" (click)="page = d.page + 1; load()">
          Next
        </button>
      </div>
    }
  `,
})
export class UsersPage {
  private readonly api = inject(AdminApi);
  q = '';
  page = 1;
  data = signal<UserList | null>(null);

  constructor() {
    this.load();
  }

  load() {
    this.api.users(this.q, this.page).subscribe((d) => this.data.set(d));
  }
}
