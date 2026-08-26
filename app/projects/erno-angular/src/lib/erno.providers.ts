import {
  EnvironmentProviders,
  ErrorHandler,
  inject,
  makeEnvironmentProviders,
  provideAppInitializer,
} from '@angular/core';
import { provideHttpClient, withInterceptors } from '@angular/common/http';

import { ErnoConfig, ERNO_CONFIG } from './erno.config';
import { ErnoAuthService } from './auth/erno-auth.service';
import { ernoHttpInterceptor } from './http/erno-http.interceptor';
import { ErnoRealtimeService } from './realtime/erno-realtime.service';
import { ErnoDatabaseService } from './sync/erno-database.service';
import { ErnoSyncService } from './sync/erno-sync.service';
import { ErnoStorageService } from './storage/erno-storage.service';
import { ErnoBillingService } from './billing/erno-billing.service';
import { ErnoShareService } from './share/erno-share.service';
import { ErnoSharedViewService } from './share/erno-shared-view.service';
import { ErnoDevMailService } from './devtools/erno-dev-mail.service';
import { ErnoDevJobsService } from './devtools/erno-dev-jobs.service';
import { ErnoDevtoolsRegistry } from './devtools/erno-devtools.registry';
import { ErnoAlertsService } from './alerts/erno-alerts.service';
import { ErnoHttpService } from './http/erno-http.service';
import { ErnoAppStateService } from './app-state/erno-app-state.service';
import { ErnoNetworkService } from './network/erno-network.service';
import { ErnoErrorReporterService } from './errors/erno-error-reporter.service';
import { ErnoErrorHandler } from './errors/erno-error-handler';

/** Standalone entry: `providers: [provideErno({ baseUrl, wsUrl })]`. */
export function provideErno(config: ErnoConfig): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: ERNO_CONFIG, useValue: config },
    provideHttpClient(withInterceptors([ernoHttpInterceptor])),
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
    ErnoDevtoolsRegistry,
    ErnoAlertsService,
    ErnoErrorReporterService,
    // Overriding ErrorHandler is intrusive, so it is a no-op pass-through
    // unless `errorReporting.key` is set. An application that wants its own
    // handler provides one after `provideErno(...)`.
    { provide: ErrorHandler, useClass: ErnoErrorHandler },
    // Without an initializer the global `error` / `unhandledrejection`
    // listeners would only attach once something first injected the reporter,
    // which might be never.
    provideAppInitializer(() => {
      inject(ErnoErrorReporterService).install();
    }),
    provideAppInitializer(() => {
      inject(ErnoDevtoolsRegistry).register(inject(ErnoDatabaseService));
    }),
  ]);
}
