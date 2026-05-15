import { Injectable } from '@angular/core';
import { CanActivate, Router, UrlTree } from '@angular/router';
import { Observable, of } from 'rxjs';
import { catchError, map } from 'rxjs/operators';
import { ErnoAuthService } from 'erno-angular';

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
        catchError(() => of(this.router.createUrlTree(['/login']))),
      );
    }
    return this.router.createUrlTree(['/login']);
  }
}
