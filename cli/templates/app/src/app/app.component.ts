import { Component, effect, inject } from '@angular/core';
import { toSignal } from '@angular/core/rxjs-interop';
import { NavigationEnd, Router } from '@angular/router';
import { filter, map } from 'rxjs/operators';
import { IonApp, IonRouterOutlet } from '@ionic/angular';
import { ErnoAuthService, ErnoDevtoolsComponent } from 'erno-angular';

/** Routes reachable without a session. */
const PUBLIC_ROUTES = ['/login', '/register', '/forgot-password', '/reset-password', '/verify-email'];
/** Routes a signed-in user has no business on. */
const GUEST_ONLY_ROUTES = ['/login', '/register'];

@Component({
  selector: 'app-root',
  templateUrl: 'app.component.html',
  imports: [IonApp, IonRouterOutlet, ErnoDevtoolsComponent],
})
export class AppComponent {
  private readonly auth = inject(ErnoAuthService);
  private readonly router = inject(Router);

  /** The current URL as a signal, so the redirect below is derived rather than timed. */
  private readonly url = toSignal(
    this.router.events.pipe(
      filter((event): event is NavigationEnd => event instanceof NavigationEnd),
      map((event): string | null => event.urlAfterRedirects),
    ),
    { initialValue: null },
  );

  constructor() {
    // Signing in or out moves the app, wherever it happened — a login page, the
    // devtools panel, or the HTTP interceptor hitting a dead session. Nothing
    // navigates by hand on the way out of an auth call.
    effect(() => {
      const state = this.auth.authState();
      const url = this.url();
      // Until the first NavigationEnd the initial guard owns the decision, and
      // `restoring` is not an answer yet.
      if (url === null || state === 'restoring') return;

      const path = url.replace(/[?#].*$/, '');
      if (state === 'anonymous' && !PUBLIC_ROUTES.includes(path)) {
        void this.router.navigate(['/login']);
      } else if (state === 'authenticated' && GUEST_ONLY_ROUTES.includes(path)) {
        void this.router.navigate(['/']);
      }
    });
  }
}
