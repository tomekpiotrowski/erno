import { Component, ChangeDetectionStrategy } from '@angular/core';
import { RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { clearBasicAuth } from '../core/auth';

@Component({
  selector: 'app-shell',
  imports: [RouterOutlet, RouterLink, RouterLinkActive],
  template: `
    <div class="shell">
      <div class="stripe" [class.prod]="env.prod">
        <span class="stripe-label">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.2"
               stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d="M12 3l9.5 17H2.5zM12 9v5M12 17h.01" />
          </svg>
          {{ env.label }}
        </span>
        <span class="stripe-note">{{ env.note }}</span>
        <span class="stripe-meta">{{ env.host }}</span>
      </div>

      <div class="layout">
        <aside>
          <div class="brand">
            <img src="assets/logo/cubeast-mark-on-dark.svg" alt="" width="26" height="26" />
            <span>
              <span class="brand-name">Cubeast</span>
              <span class="brand-sub">Monitoring</span>
            </span>
          </div>

          <nav>
            @for (it of items; track it.path) {
              <a
                class="nav"
                [routerLink]="it.path"
                routerLinkActive="on"
                [routerLinkActiveOptions]="{ exact: it.exact }"
              >
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"
                     stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
                  <path [attr.d]="it.icon" />
                </svg>
                <span>{{ it.label }}</span>
              </a>
            }
          </nav>

          <button class="ghost logout" type="button" (click)="logout()">Log out</button>
        </aside>

        <section class="page">
          <router-outlet />
        </section>
      </div>
    </div>
  `,
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: `
    .shell {
      display: flex;
      flex-direction: column;
      height: 100%;
    }
    .stripe {
      flex: 0 0 var(--cb-stripe);
      height: var(--cb-stripe);
      display: flex;
      align-items: center;
      gap: 9px;
      padding: 0 13px;
      background: color-mix(in srgb, var(--cb-info) 14%, transparent);
      border-bottom: 1px solid color-mix(in srgb, var(--cb-info) 45%, transparent);
      color: var(--cb-info);
    }
    .stripe.prod {
      background: color-mix(in srgb, var(--cb-warn) 14%, transparent);
      border-bottom-color: color-mix(in srgb, var(--cb-warn) 45%, transparent);
      color: var(--cb-warn);
    }
    .stripe-label {
      display: flex;
      align-items: center;
      gap: 6px;
      font: 700 9px / 1 var(--cb-font-text);
      letter-spacing: 0.18em;
      text-transform: uppercase;
    }
    .stripe-label svg { width: 11px; height: 11px; }
    .stripe-note {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-size: 10.5px;
      color: var(--cb-fg-dim);
    }
    .stripe-meta {
      font-family: var(--cb-font-mono);
      font-size: 10px;
      color: var(--cb-fg-dim2);
    }
    .layout {
      flex: 1;
      min-height: 0;
      display: grid;
      grid-template-columns: var(--cb-rail) minmax(0, 1fr);
    }
    aside {
      background: var(--cb-elev);
      border-right: 1px solid var(--cb-border);
      display: flex;
      flex-direction: column;
      min-height: 0;
    }
    .brand {
      display: flex;
      align-items: center;
      gap: 9px;
      padding: 13px 12px 12px;
      border-bottom: 1px solid var(--cb-border);
    }
    .brand img { display: block; width: 26px; height: 26px; }
    .brand > span { display: flex; flex-direction: column; gap: 2px; }
    .brand-name {
      font-family: var(--cb-font-display);
      font-weight: 700;
      letter-spacing: 0.03em;
      text-transform: uppercase;
      font-size: 15px;
      line-height: 1;
    }
    .brand-sub {
      font: 700 8.5px / 1 var(--cb-font-text);
      letter-spacing: 0.28em;
      text-transform: uppercase;
      color: var(--cb-accent);
    }
    nav {
      flex: 1;
      min-height: 0;
      overflow: auto;
      padding: 8px 0;
      display: flex;
      flex-direction: column;
      gap: 1px;
    }
    .nav {
      display: flex;
      align-items: center;
      gap: 9px;
      padding: 7px 11px;
      border-left: 2px solid transparent;
      color: var(--cb-fg-dim);
      font: 600 11.5px / 1 var(--cb-font-text);
      text-decoration: none;
    }
    .nav:hover { background: var(--cb-accent-soft); color: var(--cb-fg); text-decoration: none; }
    .nav.on {
      background: var(--cb-accent-soft);
      border-left-color: var(--cb-accent);
      color: var(--cb-fg);
    }
    .nav svg { width: 14px; height: 14px; flex: 0 0 auto; }
    .logout {
      margin: 10px 12px;
      justify-content: flex-start;
    }
    .page {
      min-width: 0;
      overflow: auto;
      padding: 16px;
    }
    @media (max-width: 768px) {
      .layout { grid-template-columns: 1fr; }
      aside { border-right: 0; border-bottom: 1px solid var(--cb-border); }
      nav { flex-direction: row; flex-wrap: wrap; }
    }
  `,
})
export class Shell {
  readonly env = envBanner();
  readonly items = [
    {
      path: '/issues',
      label: 'Issues',
      exact: false,
      icon: 'M8 2v3M16 2v3M4 13h3M17 13h3M5 19l2-2M19 19l-2-2M12 20a6 6 0 0 0 6-6v-2a6 6 0 0 0-12 0v2a6 6 0 0 0 6 6zM9 9h6',
    },
    {
      path: '/releases',
      label: 'Releases',
      exact: false,
      icon: 'M4 7h16M4 12h16M4 17h16M8 4v3M8 10v4M16 15v5',
    },
    {
      path: '/system',
      label: 'System',
      exact: false,
      icon: 'M4 5h16v10H4zM9 19h6M12 15v4M8 9h2M14 9h2',
    },
    {
      path: '/uptime',
      label: 'Uptime',
      exact: false,
      icon: 'M3 12h4l3-8 4 16 3-8h4',
    },
    {
      path: '/performance',
      label: 'Performance',
      exact: false,
      icon: 'M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 8v4.5l3 2',
    },
    {
      path: '/business',
      label: 'Statistics',
      exact: false,
      icon: 'M4 20V10m5 10V4m5 16v-7m5 7V7',
    },
    {
      path: '/alerts',
      label: 'Alerts',
      exact: false,
      icon: 'M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 0 1-3.4 0',
    },
    {
      path: '/status',
      label: 'Status page',
      exact: false,
      icon: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18zM12 8v4l3 2',
    },
  ];

  logout() {
    clearBasicAuth();
    location.href = '/login';
  }
}

function envBanner() {
  const host = typeof location === 'undefined' ? 'local' : location.hostname;
  const local = host === 'localhost' || host === '127.0.0.1' || host.endsWith('.local');
  return local
    ? { prod: false, label: 'Development', note: 'Local collector. Safe to break.', host }
    : { prod: true, label: 'Production', note: 'Live diagnostics — triage actions are logged.', host };
}
