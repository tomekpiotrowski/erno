import { inject } from '@angular/core';
import { HttpInterceptorFn } from '@angular/common/http';
import { CanActivateFn, Router } from '@angular/router';

// Distinct from the admin console's key: this is a different deployment
// with different credentials, and they must not share a session.
const KEY = 'erno-monitoring-auth';

export function getBasicAuth(): string | null {
  return sessionStorage.getItem(KEY);
}

export function setBasicAuth(user: string, password: string): void {
  sessionStorage.setItem(KEY, btoa(`${user}:${password}`));
}

export function clearBasicAuth(): void {
  sessionStorage.removeItem(KEY);
}

export const authInterceptor: HttpInterceptorFn = (req, next) => {
  const token = getBasicAuth();
  if (
    token &&
    (req.url.startsWith('/api/collector') || req.url.startsWith('/prometheus'))
  ) {
    req = req.clone({ setHeaders: { Authorization: `Basic ${token}` } });
  }
  return next(req);
};

export const authGuard: CanActivateFn = () => {
  if (getBasicAuth()) {
    return true;
  }
  return inject(Router).createUrlTree(['/login']);
};
