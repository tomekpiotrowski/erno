import { ApplicationConfig, ErrorHandler, provideZonelessChangeDetection } from '@angular/core';
import { provideHttpClient, withInterceptors, withXhr } from '@angular/common/http';
import { provideRouter } from '@angular/router';
import { routes } from './app.routes';
import { authInterceptor } from './core/auth';
import { MonitoringErrorHandler } from './core/error-reporter';

export const appConfig: ApplicationConfig = {
  providers: [
    provideZonelessChangeDetection(),
    provideHttpClient(withXhr(), withInterceptors([authInterceptor])),
    provideRouter(routes),
    // Replaces Angular's default handler, which only logs. This console
    // reports its own crashes to the collector it is a front end for.
    { provide: ErrorHandler, useClass: MonitoringErrorHandler },
  ],
};
