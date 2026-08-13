import { Component } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { clearBasicAuth } from '../core/auth';

@Component({
  selector: 'app-shell',
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  template: `
    <div class="layout">
      <aside>
        <strong>Erno</strong>
        <nav>
          <a routerLink="/" routerLinkActive="on" [routerLinkActiveOptions]="{ exact: true }">Dashboard</a>
          <a routerLink="/users" routerLinkActive="on">Users</a>
          <a routerLink="/jobs" routerLinkActive="on">Jobs</a>
          <a routerLink="/emails" routerLinkActive="on">Emails</a>
          <a routerLink="/events" routerLinkActive="on">Events</a>
          <a routerLink="/database" routerLinkActive="on">Database</a>
          <a routerLink="/performance" routerLinkActive="on">Performance</a>
          <a routerLink="/business" routerLinkActive="on">Business</a>
        </nav>
        <button (click)="logout()">Log out</button>
      </aside>
      <section>
        <router-outlet />
      </section>
    </div>
  `,
  styles: `
    .layout { display: grid; grid-template-columns: 200px 1fr; min-height: 100%; }
    aside {
      border-right: 1px solid var(--line);
      padding: 1.1rem 0.9rem;
      display: flex;
      flex-direction: column;
      gap: 1rem;
    }
    nav { display: grid; gap: 0.2rem; }
    nav a { padding: 0.35rem 0.5rem; border-radius: 6px; color: var(--text); }
    nav a.on { background: #243044; color: var(--accent); }
    section { padding: 1.2rem 1.4rem; overflow: auto; }
    @media (max-width: 720px) {
      .layout { grid-template-columns: 1fr; }
      aside { border-right: 0; border-bottom: 1px solid var(--line); }
      nav { grid-auto-flow: column; overflow: auto; }
    }
  `,
})
export class Shell {
  logout() {
    clearBasicAuth();
    location.href = '/login';
  }
}
