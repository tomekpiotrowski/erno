import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  OnInit,
  computed,
  inject,
  output,
  signal,
} from '@angular/core';
import { ErnoAuthService } from '../../auth/erno-auth.service';
import { ERNO_DEVTOOLS_STYLES } from '../erno-devtools.styles';
import {
  StatusRow,
  decodeJwtClaims,
  formatClock,
  formatExpiry,
  tokenFingerprint,
  toneColor,
} from '../erno-devtools.util';

const ACCESS_KEY = 'erno_access_token';
const SEED_EMAIL = 'dev@example.com';
const SEED_PASSWORD = 'password';

@Component({
  selector: 'erno-devtools-auth-tab',
  imports: [],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { style: 'display: contents' },
  styles: [ERNO_DEVTOOLS_STYLES],
  template: `
    @for (row of rows(); track row.key) {
      <div class="srow">
        <span class="skey">{{ row.key }}</span>
        <span class="sval">
          <span class="smain" [style.color]="toneColor(row.tone)">{{ row.val }}</span>
          @if (row.detail) {
            <span class="sdetail">{{ row.detail }}</span>
          }
        </span>
        <span class="smeta">{{ row.meta }}</span>
      </div>
    }
    <div class="jbar">
      <input
        class="filter"
        placeholder="email"
        [value]="email()"
        (input)="email.set($any($event.target).value)"
      />
    </div>
    <div class="jbar">
      <input
        class="filter"
        type="password"
        placeholder="password"
        [value]="password()"
        (input)="password.set($any($event.target).value)"
      />
    </div>
    <div class="acts">
      <button type="button" class="primary" (click)="login()" [disabled]="busy()">
        {{ user() ? 're-login' : 'sign in' }}
      </button>
      <button type="button" class="ghost sm" (click)="logout()" [disabled]="!user() || busy()">
        logout
      </button>
      <button type="button" class="ghost sm" (click)="refresh()" [disabled]="!refreshPresent() || busy()">
        refresh
      </button>
      <button type="button" class="ghost sm" (click)="dropAccess()" [disabled]="!accessPresent()">
        drop access
      </button>
      <button type="button" class="ghost sm" (click)="copyId()" [disabled]="!user()">
        copy id
      </button>
    </div>
  `,
})
export class ErnoDevtoolsAuthTab implements OnInit {
  private readonly auth = inject(ErnoAuthService);
  private readonly destroyRef = inject(DestroyRef);

  readonly note = output<string>();

  readonly email = signal(SEED_EMAIL);
  readonly password = signal(SEED_PASSWORD);
  /** The live session, straight off the service. */
  readonly user = this.auth.currentUser;
  readonly accessPresent = signal(false);
  readonly refreshPresent = signal(false);
  readonly busy = signal(false);
  readonly now = signal(Date.now());

  readonly rows = computed((): StatusRow[] => {
    const user = this.user();
    const claims = decodeJwtClaims(this.auth.accessToken);
    const access = this.accessPresent();
    const refresh = this.refreshPresent();
    const now = this.now();
    return [
      {
        key: 'user',
        val: user ? user.email : 'signed out',
        tone: user ? 'ok' : 'muted',
        meta: user ? user.id.replace(/-/g, '').slice(0, 8) : '',
        detail: user ? user.id : 'login as the erno dev seed user, or any local account',
      },
      {
        key: 'access',
        val: access ? formatExpiry(claims?.exp, now) : 'missing',
        tone: !access ? 'err' : formatExpiry(claims?.exp, now) === 'expired' ? 'err' : 'ok',
        meta: 'sessionStorage',
        detail: access
          ? `ver ${claims?.ver ?? '—'} · iat ${claims?.iat ? formatClock(claims.iat * 1000) : '—'}`
          : 'drop this to exercise the 401 refresh path',
      },
      {
        key: 'refresh',
        val: refresh ? `present · ${tokenFingerprint(this.auth.refreshToken)}` : 'missing',
        tone: refresh ? 'ok' : 'err',
        meta: 'localStorage',
        detail: refresh ? 'raw token is not shown' : 'silent re-auth will fail until you sign in',
      },
    ];
  });

  readonly toneColor = toneColor;

  ngOnInit(): void {
    this.syncTokens();
    const tick = setInterval(() => {
      this.now.set(Date.now());
      this.syncTokens();
    }, 1000);
    this.destroyRef.onDestroy(() => clearInterval(tick));
  }

  login(): void {
    if (this.busy()) return;
    this.busy.set(true);
    this.auth.login(this.email(), this.password()).subscribe({
      next: () => {
        this.busy.set(false);
        this.syncTokens();
        this.note.emit('signed in');
      },
      error: () => {
        this.busy.set(false);
        this.note.emit('login failed');
      },
    });
  }

  logout(): void {
    this.busy.set(true);
    this.auth.logout().subscribe({
      next: () => {
        this.busy.set(false);
        this.syncTokens();
        this.note.emit('signed out');
      },
      error: () => {
        this.busy.set(false);
        this.syncTokens();
        this.note.emit('signed out');
      },
    });
  }

  refresh(): void {
    this.busy.set(true);
    this.auth.refresh().subscribe({
      next: () => {
        this.busy.set(false);
        this.syncTokens();
        this.note.emit('refreshed');
      },
      error: () => {
        this.busy.set(false);
        this.note.emit('refresh failed');
      },
    });
  }

  dropAccess(): void {
    sessionStorage.removeItem(ACCESS_KEY);
    this.syncTokens();
    this.note.emit('access token dropped');
  }

  copyId(): void {
    const id = this.user()?.id;
    if (!id) return;
    void navigator.clipboard.writeText(id).then(
      () => this.note.emit('copied user id'),
      () => this.note.emit('copy failed'),
    );
  }

  private syncTokens(): void {
    this.accessPresent.set(!!this.auth.accessToken);
    this.refreshPresent.set(!!this.auth.refreshToken);
  }
}
