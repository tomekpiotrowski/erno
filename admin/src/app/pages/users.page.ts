import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { AdminApi, UserList } from '../core/api';

@Component({
  selector: 'app-users',
  imports: [FormsModule, RouterLink],
  template: `
    <div class="stack">
      <header class="head">
        <div>
          <h1>Users</h1>
          <p class="sub">One row per account. Search is exact-substring on email.</p>
        </div>
      </header>

      <div class="toolbar">
        <input [(ngModel)]="q" (keyup.enter)="load()" placeholder="Search email" />
        <button type="button" (click)="load()">Search</button>
      </div>

      @if (data(); as d) {
        <section class="panel flush">
          <table>
            <thead>
              <tr>
                <th>Email</th>
                <th>Plan</th>
                <th>Verified</th>
                <th>Last active</th>
              </tr>
            </thead>
            <tbody>
              @for (u of d.users; track u.id) {
                <tr>
                  <td><a [routerLink]="['/users', u.id]">{{ u.email }}</a></td>
                  <td>{{ u.subscription_plan || '—' }}</td>
                  <td>
                    <span class="status" [class.good]="!!u.email_verified_at" [class.warn]="!u.email_verified_at">
                      {{ u.email_verified_at ? 'Verified' : 'Unverified' }}
                    </span>
                  </td>
                  <td class="id">{{ u.last_active_at || '—' }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
        <div class="toolbar">
          <span class="muted">{{ d.total }} users · page {{ d.page }}</span>
          <button type="button" [disabled]="d.page <= 1" (click)="page = d.page - 1; load()">Prev</button>
          <button type="button" [disabled]="d.page * d.per_page >= d.total" (click)="page = d.page + 1; load()">
            Next
          </button>
        </div>
      }
    </div>
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
