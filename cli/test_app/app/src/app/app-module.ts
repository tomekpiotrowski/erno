import { NgModule, provideZonelessChangeDetection } from '@angular/core';
import { BrowserModule } from '@angular/platform-browser';
import { ErnoModule } from 'erno-angular';

import { AppRoutingModule } from './app-routing-module';
import { App } from './app';

@NgModule({
  declarations: [App],
  imports: [
    BrowserModule,
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
