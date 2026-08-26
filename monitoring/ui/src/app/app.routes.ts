import { Routes } from '@angular/router';
import { authGuard } from './core/auth';
import { Shell } from './shell/shell';
import { LoginPage } from './login/login.page';
import { IssuesPage } from './pages/issues.page';
import { IssueDetailPage } from './pages/issue-detail.page';
import { ReleasesPage } from './pages/releases.page';
import { SystemPage } from './pages/system.page';
import { UptimePage } from './pages/uptime.page';
import { StatusPage } from './pages/status.page';
import { AlertsPage } from './pages/alerts.page';
import { PerformancePage } from './pages/performance.page';
import { TraceDetailPage } from './pages/trace-detail.page';
import { LogsPage } from './pages/logs.page';
import { BusinessPage } from './pages/business.page';

export const routes: Routes = [
  { path: 'login', component: LoginPage },
  {
    path: '',
    component: Shell,
    canActivate: [authGuard],
    children: [
      { path: '', pathMatch: 'full', redirectTo: 'issues' },
      { path: 'issues', component: IssuesPage },
      { path: 'issues/:id', component: IssueDetailPage },
      { path: 'releases', component: ReleasesPage },
      { path: 'system', component: SystemPage },
      { path: 'uptime', component: UptimePage },
      { path: 'status', component: StatusPage },
      { path: 'alerts', component: AlertsPage },
      { path: 'performance', component: PerformancePage },
      { path: 'performance/traces/:id', component: TraceDetailPage },
      { path: 'logs', component: LogsPage },
      { path: 'business', component: BusinessPage },
    ],
  },
];
