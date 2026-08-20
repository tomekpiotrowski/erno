import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { HTTP_INTERCEPTORS, provideHttpClient, withInterceptorsFromDi } from '@angular/common/http';

import { ErnoConfig, ERNO_CONFIG } from './erno.config';
import { ErnoAuthService } from './auth/erno-auth.service';
import { ErnoHttpInterceptor } from './http/erno-http.interceptor';
import { ErnoRealtimeService } from './realtime/erno-realtime.service';
import { ErnoDatabaseService } from './sync/erno-database.service';
import { ErnoSyncService } from './sync/erno-sync.service';
import { ErnoStorageService } from './storage/erno-storage.service';
import { ErnoBillingService } from './billing/erno-billing.service';
import { ErnoShareService } from './share/erno-share.service';
import { ErnoSharedViewService } from './share/erno-shared-view.service';
import { ErnoDevMailService } from './devtools/erno-dev-mail.service';
import { ErnoDevJobsService } from './devtools/erno-dev-jobs.service';
import { ErnoAlertsService } from './alerts/erno-alerts.service';
import { ErnoHttpService } from './http/erno-http.service';
import { ErnoAppStateService } from './app-state/erno-app-state.service';
import { ErnoNetworkService } from './network/erno-network.service';

/** Standalone entry: `providers: [provideErno({ baseUrl, wsUrl })]`. */
export function provideErno(config: ErnoConfig): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: ERNO_CONFIG, useValue: config },
    provideHttpClient(withInterceptorsFromDi()),
    { provide: HTTP_INTERCEPTORS, useClass: ErnoHttpInterceptor, multi: true },
    ErnoHttpService,
    ErnoAuthService,
    ErnoAppStateService,
    ErnoNetworkService,
    ErnoRealtimeService,
    ErnoDatabaseService,
    ErnoSyncService,
    ErnoStorageService,
    ErnoBillingService,
    ErnoShareService,
    ErnoSharedViewService,
    ErnoDevMailService,
    ErnoDevJobsService,
    ErnoAlertsService,
  ]);
}
