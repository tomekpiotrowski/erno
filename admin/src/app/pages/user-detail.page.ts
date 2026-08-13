import { Component, inject, signal } from '@angular/core';
import { ActivatedRoute, Router } from '@angular/router';
import { FormsModule } from '@angular/forms';
import { AdminApi, UserDetail } from '../core/api';

@Component({
  selector: 'app-user-detail',
  imports: [FormsModule],
  template: `
    @if (data(); as d) {
      <h1>{{ d.user.email }}</h1>
      <p class="muted">{{ d.user.id }} · created {{ d.user.created_at }}</p>
      <p>Verified: {{ d.user.email_verified_at || 'no' }}</p>
      <p>Last active: {{ d.user.last_active_at || '—' }}</p>
      <p>OAuth: {{ d.oauth_providers.join(', ') || '—' }}</p>
      <p>
        Current:
        {{ d.subscription ? d.subscription.sub_type + ' / ' + d.subscription.plan : 'none' }}
      </p>
      <div class="toolbar">
        <button (click)="activate()">Activate</button>
        <input [(ngModel)]="plan" placeholder="plan" style="max-width:120px" />
        <input [(ngModel)]="days" type="number" style="max-width:80px" />
        <button (click)="gift()">Gift</button>
        <button class="danger" (click)="remove()">Delete</button>
      </div>
      <h2>History</h2>
      <table>
        <tr><th>Type</th><th>Plan</th><th>Status</th><th>Expiry</th></tr>
        @for (s of d.subscription_history; track $index) {
          <tr>
            <td>{{ s.sub_type }}</td>
            <td>{{ s.plan }}</td>
            <td>{{ s.status }}</td>
            <td>{{ s.expiry }}</td>
          </tr>
        }
      </table>
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

  remove() {
    const id = this.route.snapshot.paramMap.get('id')!;
    if (!confirm('Delete this user permanently?')) return;
    this.api.deleteUser(id).subscribe(() => void this.router.navigateByUrl('/users'));
  }
}
