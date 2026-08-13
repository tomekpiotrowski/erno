import { inject } from '@angular/core';
import { HttpInterceptorFn } from '@angular/common/http';
import { CanActivateFn, Router } from '@angular/router';

const KEY = 'erno-admin-auth';

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
    (req.url.startsWith('/admin/api') || req.url.startsWith('/prometheus'))
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
