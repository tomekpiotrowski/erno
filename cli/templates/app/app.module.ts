import { NgModule, provideZonelessChangeDetection } from '@angular/core';
import { BrowserModule } from '@angular/platform-browser';
import { ReactiveFormsModule } from '@angular/forms';
import { ErnoModule } from 'erno-angular';

import { AppRoutingModule } from './app-routing-module';
import { App } from './app';
import { HomeComponent } from './home/home.component';
import { LoginComponent } from './auth/login/login.component';
import { RegisterComponent } from './auth/register/register.component';
import { ForgotPasswordComponent } from './auth/forgot-password/forgot-password.component';
import { ResetPasswordComponent } from './auth/reset-password/reset-password.component';
import { VerifyEmailComponent } from './auth/verify-email/verify-email.component';

@NgModule({
  declarations: [
    App,
    HomeComponent,
    LoginComponent,
    RegisterComponent,
    ForgotPasswordComponent,
    ResetPasswordComponent,
    VerifyEmailComponent,
  ],
  imports: [
    BrowserModule,
    ReactiveFormsModule,
    AppRoutingModule,
    ErnoModule.forRoot({
      baseUrl: 'http://localhost:3000',
      wsUrl: 'ws://localhost:3000',
    }),
  ],
  providers: [provideZonelessChangeDetection()],
  bootstrap: [App],
})
export class AppModule {}
