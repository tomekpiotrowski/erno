import { Routes } from '@angular/router';
import { authGuard } from './core/auth';
import { LoginPage } from './login/login.page';
import { Shell } from './shell/shell';
import { DashboardPage } from './pages/dashboard.page';
import { UsersPage } from './pages/users.page';
import { UserDetailPage } from './pages/user-detail.page';
import { JobsPage } from './pages/jobs.page';
import { JobDetailPage } from './pages/job-detail.page';
import { EmailsPage } from './pages/emails.page';
import { EventsPage } from './pages/events.page';
import { DatabasePage } from './pages/database.page';

export const routes: Routes = [
  { path: 'login', component: LoginPage },
  {
    path: '',
    component: Shell,
    canActivate: [authGuard],
    children: [
      { path: '', component: DashboardPage },
      { path: 'users', component: UsersPage },
      { path: 'users/:id', component: UserDetailPage },
      { path: 'jobs', component: JobsPage },
      { path: 'jobs/:id', component: JobDetailPage },
      { path: 'emails', component: EmailsPage },
      { path: 'events', component: EventsPage },
      { path: 'database', component: DatabasePage },
    ],
  },
];
