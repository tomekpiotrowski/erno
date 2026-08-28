import { inject } from '@angular/core';
import { CanActivateFn, Router } from '@angular/router';
import { filter, map, take } from 'rxjs/operators';
import { ErnoAuthService } from 'erno-angular';

/**
 * Guards a route on `ErnoAuthService.authState$` rather than on the tokens in
 * storage, so there is one answer to "is there a session" and everything reads
 * it. On a cold load the state sits at `restoring` until the silent refresh
 * comes back — waiting for that is what keeps a reload from flashing /login.
 */
export const authGuard: CanActivateFn = () => {
  const auth = inject(ErnoAuthService);
  const router = inject(Router);

  return auth.authState$.pipe(
    filter(state => state !== 'restoring'),
    take(1),
    map(state => (state === 'authenticated' ? true : router.createUrlTree(['/login']))),
  );
};
