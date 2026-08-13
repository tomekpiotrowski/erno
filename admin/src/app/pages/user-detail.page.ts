import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { FormsModule } from '@angular/forms';
import { AdminApi, UserDetail } from '../core/api';

@Component({
  selector: 'app-user-detail',
  imports: [FormsModule, RouterLink],
  template: `
    @if (data(); as d) {
      <div class="stack">
        <header class="head">
          <div>
            <a routerLink="/users" class="muted">← Users</a>
            <h1>{{ d.user.email }}</h1>
            <p class="sub id">{{ d.user.id }}</p>
          </div>
        </header>

        <dl class="kv">
          <dt>Created</dt>
          <dd class="id">{{ d.user.created_at }}</dd>
          <dt>Verified</dt>
          <dd>
            <span class="status" [class.good]="!!d.user.email_verified_at" [class.warn]="!d.user.email_verified_at">
              {{ d.user.email_verified_at || 'no' }}
            </span>
          </dd>
          <dt>Last active</dt>
          <dd class="id">{{ d.user.last_active_at || '—' }}</dd>
          <dt>OAuth</dt>
          <dd>{{ d.oauth_providers.join(', ') || '—' }}</dd>
          <dt>Current</dt>
          <dd>{{ d.subscription ? d.subscription.sub_type + ' / ' + d.subscription.plan : 'none' }}</dd>
        </dl>

        <div class="toolbar">
          <button type="button" (click)="activate()">Activate</button>
          <input [(ngModel)]="plan" placeholder="plan" style="max-width:120px" />
          <input [(ngModel)]="days" type="number" style="max-width:80px" />
          <button type="button" (click)="gift()">Gift</button>
          <button type="button" class="danger" (click)="openDelete()">Delete</button>
        </div>

        <section class="panel flush">
          <header class="phead"><span class="eyebrow">History</span></header>
          <table>
            <thead>
              <tr><th>Type</th><th>Plan</th><th>Status</th><th>Expiry</th></tr>
            </thead>
            <tbody>
              @for (s of d.subscription_history; track $index) {
                <tr>
                  <td>{{ s.sub_type }}</td>
                  <td>{{ s.plan }}</td>
                  <td>{{ s.status }}</td>
                  <td class="id">{{ s.expiry }}</td>
                </tr>
              }
            </tbody>
          </table>
        </section>
      </div>

      @if (confirmOpen()) {
        <div class="overlay" (click)="closeDelete()" (keydown.escape)="closeDelete()">
          <div
            class="dialog"
            role="dialog"
            aria-modal="true"
            aria-labelledby="delete-title"
            (click)="$event.stopPropagation()"
          >
            <h2 id="delete-title">Delete this user</h2>
            <p class="muted">
              This permanently erases the account and related data. Type
              <span class="mono">{{ d.user.email }}</span> to confirm.
            </p>
            <label class="eyebrow" for="confirm-email">Email</label>
            <input
              id="confirm-email"
              name="confirm-email"
              class="mono"
              [ngModel]="confirmEmail()"
              (ngModelChange)="confirmEmail.set($event)"
              autocomplete="off"
              autofocus
              (keydown.enter)="confirmDelete(d)"
            />
            @if (deleteError()) {
              <p class="error">{{ deleteError() }}</p>
            }
            <div class="actions">
              <button type="button" class="ghost" (click)="closeDelete()" [disabled]="deleting()">
                Cancel
              </button>
              <button
                type="button"
                class="danger"
                [disabled]="!matches(d.user.email) || deleting()"
                (click)="confirmDelete(d)"
              >
                {{ deleting() ? 'Deleting…' : 'Delete user' }}
              </button>
            </div>
          </div>
        </div>
      }
    }
  `,
})
export class UserDetailPage {
  private readonly api = inject(AdminApi);
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);
  data = signal<UserDetail | null>(null);
  plan = 'pro';
  days = 30;
  confirmOpen = signal(false);
  confirmEmail = signal('');
  deleting = signal(false);
  deleteError = signal('');

  constructor() {
    const id = this.route.snapshot.paramMap.get('id')!;
    this.api.user(id).subscribe((d) => this.data.set(d));
  }

  activate() {
    const id = this.route.snapshot.paramMap.get('id')!;
    this.api.activate(id).subscribe((d) => this.data.set(d));
  }

  gift() {
    const id = this.route.snapshot.paramMap.get('id')!;
    this.api.gift(id, this.plan, this.days).subscribe((d) => this.data.set(d));
  }

  matches(email: string) {
    return this.confirmEmail().trim() === email;
  }

  openDelete() {
    this.confirmEmail.set('');
    this.deleteError.set('');
    this.deleting.set(false);
    this.confirmOpen.set(true);
  }

  closeDelete() {
    if (this.deleting()) return;
    this.confirmOpen.set(false);
  }

  confirmDelete(d: UserDetail) {
    if (!this.matches(d.user.email) || this.deleting()) return;
    this.deleting.set(true);
    this.deleteError.set('');
    this.api.deleteUser(d.user.id).subscribe({
      next: () => void this.router.navigateByUrl('/users'),
      error: () => {
        this.deleting.set(false);
        this.deleteError.set('Delete failed.');
      },
    });
  }
}
