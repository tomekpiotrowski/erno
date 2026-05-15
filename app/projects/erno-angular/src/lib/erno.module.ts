import { NgModule, ModuleWithProviders } from '@angular/core';
import { CommonModule } from '@angular/common';
import { HTTP_INTERCEPTORS, HttpClientModule } from '@angular/common/http';

import { ErnoConfig, ERNO_CONFIG } from './erno.config';
import { ErnoAuthService } from './auth/erno-auth.service';
import { ErnoHttpInterceptor } from './http/erno-http.interceptor';
import { ErnoRealtimeService } from './realtime/erno-realtime.service';
import { ErnoDatabaseService } from './sync/erno-database.service';
import { ErnoSyncService } from './sync/erno-sync.service';
import { ErnoStorageService } from './storage/erno-storage.service';
import { ErnoBillingService } from './billing/erno-billing.service';
import { ErnoDevtoolsComponent } from './devtools/erno-devtools.component';
import { ErnoDevMailService } from './devtools/erno-dev-mail.service';
import { ErnoAlertsService } from './alerts/erno-alerts.service';

@NgModule({
  imports: [CommonModule, HttpClientModule],
  declarations: [ErnoDevtoolsComponent],
  exports: [ErnoDevtoolsComponent],
})
export class ErnoModule {
  static forRoot(config: ErnoConfig): ModuleWithProviders<ErnoModule> {
    return {
      ngModule: ErnoModule,
      providers: [
        { provide: ERNO_CONFIG, useValue: config },
        { provide: HTTP_INTERCEPTORS, useClass: ErnoHttpInterceptor, multi: true },
        ErnoAuthService,
        ErnoRealtimeService,
        ErnoDatabaseService,
        ErnoSyncService,
        ErnoStorageService,
        ErnoBillingService,
        ErnoDevMailService,
        ErnoAlertsService,
      ],
    };
  }
}
