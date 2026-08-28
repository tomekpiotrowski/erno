import { Routes } from '@angular/router';
import { authGuard } from './auth/auth.guard';
import { HomePage } from './home/home.page';
import { LoginComponent } from './auth/login/login.component';
import { RegisterComponent } from './auth/register/register.component';
import { ForgotPasswordComponent } from './auth/forgot-password/forgot-password.component';
import { ResetPasswordComponent } from './auth/reset-password/reset-password.component';
import { VerifyEmailComponent } from './auth/verify-email/verify-email.component';

export const routes: Routes = [
  { path: '', component: HomePage, canActivate: [authGuard] },
  { path: 'login', component: LoginComponent },
  { path: 'register', component: RegisterComponent },
  { path: 'forgot-password', component: ForgotPasswordComponent },
  { path: 'reset-password', component: ResetPasswordComponent },
  { path: 'verify-email', component: VerifyEmailComponent },
  { path: '**', redirectTo: '' },
];
