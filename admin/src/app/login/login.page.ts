import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { HttpClient } from '@angular/common/http';
import { setBasicAuth } from '../core/auth';

@Component({
  selector: 'app-login',
  imports: [FormsModule],
  template: `
    <main class="login">
      <form (ngSubmit)="submit()">
        <h1>Erno admin</h1>
        <label>Username <input name="user" [(ngModel)]="user" autocomplete="username" /></label>
        <label
          >Password
          <input
            name="password"
            type="password"
            [(ngModel)]="password"
            autocomplete="current-password"
        /></label>
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
    }
    form {
      width: min(360px, 92vw);
      display: grid;
      gap: 0.8rem;
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 12px;
      padding: 1.4rem;
    }
    label { display: grid; gap: 0.3rem; font-size: 0.9rem; color: var(--muted); }
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
      error: () => {
        this.error.set('Sign-in failed. Check username and password.');
      },
    });
  }
}
