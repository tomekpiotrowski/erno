import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { HttpClient, HttpErrorResponse } from '@angular/common/http';
import { setBasicAuth } from '../core/auth';

@Component({
  selector: 'app-login',
  imports: [FormsModule],
  template: `
    <main class="login">
      <form (ngSubmit)="submit()">
        <div class="brand">
          <img src="assets/logo/cubeast-mark-on-dark.svg" alt="" width="28" height="28" />
          <span>
            <span class="brand-name">Cubeast</span>
            <span class="brand-sub">Admin</span>
          </span>
        </div>
        <h1>Sign in</h1>
        <p class="sub">Operator console. Every write is logged.</p>
        <label class="eyebrow" for="user">Username</label>
        <input id="user" name="user" [(ngModel)]="user" autocomplete="username" />
        <label class="eyebrow" for="password">Password</label>
        <input
          id="password"
          name="password"
          type="password"
          [(ngModel)]="password"
          autocomplete="current-password"
        />
        @if (error()) {
          <p class="error">{{ error() }}</p>
        }
        <button class="primary" type="submit">Sign in</button>
      </form>
    </main>
  `,
  styles: `
    .login {
      min-height: 100%;
      display: grid;
      place-items: center;
      padding: 24px;
    }
    form {
      width: min(360px, 92vw);
      display: grid;
      gap: 7px;
      background: var(--cb-elev);
      border: 1px solid var(--cb-border);
      border-radius: 5px;
      padding: 22px 20px;
    }
    .brand {
      display: flex;
      align-items: center;
      gap: 9px;
      margin-bottom: 6px;
    }
    .brand img { display: block; }
    .brand > span { display: flex; flex-direction: column; gap: 2px; }
    .brand-name {
      font-family: var(--cb-font-display);
      font-weight: 700;
      letter-spacing: 0.03em;
      text-transform: uppercase;
      font-size: 16px;
      line-height: 1;
    }
    .brand-sub {
      font: 700 8.5px / 1 var(--cb-font-text);
      letter-spacing: 0.28em;
      text-transform: uppercase;
      color: var(--cb-accent);
    }
    h1 { margin: 4px 0 0; font-size: 22px; }
    .sub { margin: 0 0 8px; font-size: 11.5px; color: var(--cb-fg-dim); }
    input { width: 100%; }
    .primary { margin-top: 8px; width: 100%; }
    .eyebrow { margin-top: 4px; }
  `,
})
export class LoginPage {
  private readonly http = inject(HttpClient);
  private readonly router = inject(Router);
  user = 'admin';
  password = 'admin';
  error = signal('');

  submit() {
    this.error.set('');
    setBasicAuth(this.user, this.password);
    this.http.get('/admin/api/dashboard').subscribe({
      next: () => void this.router.navigateByUrl('/'),
      error: (err: HttpErrorResponse) => {
        this.error.set(loginError(err));
      },
    });
  }
}

function loginError(err: HttpErrorResponse): string {
  switch (err.status) {
    case 401:
      return 'Sign-in failed. Check username and password.';
    case 404:
    case 503:
      return 'Admin API is not enabled on this server.';
    case 429:
      return 'Too many attempts. Wait a moment and try again.';
    case 0:
    case 500:
    case 502:
    case 504:
      return 'API is unreachable. Is the server running?';
    default:
      return `Sign-in failed (${err.status || 'network error'}).`;
  }
}
