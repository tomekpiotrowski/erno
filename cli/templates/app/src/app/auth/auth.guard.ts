import { Injectable } from '@angular/core';
import { CanActivate, Router, UrlTree } from '@angular/router';
import { Observable, of } from 'rxjs';
import { catchError, map } from 'rxjs/operators';
import { ErnoAuthService, isFatalRefreshError } from 'erno-angular';

@Injectable({ providedIn: 'root' })
export class AuthGuard implements CanActivate {
  constructor(private auth: ErnoAuthService, private router: Router) {}

  canActivate(): Observable<boolean | UrlTree> | boolean | UrlTree {
    if (this.auth.accessToken) {
      return true;
    }
    if (this.auth.refreshToken) {
      return this.auth.refresh().pipe(
        map(() => true),
        catchError(err => {
          if (!isFatalRefreshError(err) && this.auth.refreshToken) {
            return of(true);
          }
          return of(this.router.createUrlTree(['/login']));
        }),
      );
    }
    return this.router.createUrlTree(['/login']);
  }
}
