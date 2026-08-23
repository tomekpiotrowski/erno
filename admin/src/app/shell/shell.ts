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
              <span class="brand-sub">Admin</span>
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

          <a class="nav external" [href]="monitoringUrl" target="_blank" rel="noopener">
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.9"
                 stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
              <path d="M4 20V10m5 10V4m5 16v-7m5 7V7" />
            </svg>
            <span>Monitoring ↗</span>
          </a>

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
    .nav.external { color: var(--cb-fg-dim2); }
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

  /**
   * The monitoring console, which owns error reporting, uptime, alerts and the
   * performance and statistics dashboards that used to live here.
   *
   * A separate deployment, so this is a plain link rather than a route. On
   * localhost it is the dev port; elsewhere it is assumed to be a `monitoring.`
   * subdomain, which an operator can correct if their DNS differs.
   */
  readonly monitoringUrl = monitoringConsoleUrl();
  readonly items = [
    { path: '/', label: 'Overview', exact: true, icon: 'M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z' },
    { path: '/users', label: 'Users', exact: false, icon: 'M16 20v-1.5a4 4 0 0 0-4-4H7a4 4 0 0 0-4 4V20M9.5 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7M21 20v-1.5a4 4 0 0 0-3-3.87M16.5 4.13a4 4 0 0 1 0 7.75' },
    { path: '/jobs', label: 'Jobs', exact: false, icon: 'M12 15.5a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.11a1.7 1.7 0 0 0-1.1-1.56 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.7 1.7 0 0 0 4.6 15a1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.11A1.7 1.7 0 0 0 4.6 9a1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.7 1.7 0 0 0 9 4.6h.07A1.7 1.7 0 0 0 10.1 3.04V3a2 2 0 1 1 4 0v.11a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.7 1.7 0 0 0 19.4 9v.07a1.7 1.7 0 0 0 1.56 1.03H21a2 2 0 1 1 0 4h-.11a1.7 1.7 0 0 0-1.49 1.03z' },
    { path: '/emails', label: 'Emails', exact: false, icon: 'M3 6h18v12H3zM3 7l9 6 9-6' },
    { path: '/events', label: 'Audit log', exact: false, icon: 'M8 4h9l3 3v13H8zM8 9H4v11h4M12 11h5M12 15h5' },
    { path: '/database', label: 'Database', exact: false, icon: 'M12 3c4.4 0 8 1.3 8 3s-3.6 3-8 3-8-1.3-8-3 3.6-3 8-3Z M4 6v6c0 1.7 3.6 3 8 3s8-1.3 8-3V6 M4 12v6c0 1.7 3.6 3 8 3s8-1.3 8-3v-6' },
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
    ? { prod: false, label: 'Development', note: 'Local data. Safe to break.', host }
    : { prod: true, label: 'Production', note: 'Live user data — every write is logged.', host };
}

function monitoringConsoleUrl(): string {
  if (typeof location === 'undefined') {
    return 'http://localhost:4400';
  }
  const host = location.hostname;
  if (host === 'localhost' || host === '127.0.0.1' || host.endsWith('.local')) {
    return 'http://localhost:4400';
  }
  return `${location.protocol}//monitoring.${host.replace(/^admin\./, '')}`;
}
