import { ChangeDetectionStrategy, Component, inject, OnInit, signal } from '@angular/core';
import { ActivatedRoute, Router, RouterLink } from '@angular/router';
import { IonCard, IonCardContent, IonCardHeader, IonCardTitle, IonContent, IonText } from '@ionic/angular';
import { ErnoAuthService, ErnoAlertsService } from 'erno-angular';

@Component({
  selector: 'app-verify-email',
  templateUrl: './verify-email.component.html',
  imports: [RouterLink, IonContent, IonCard, IonCardHeader, IonCardTitle, IonCardContent, IonText],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class VerifyEmailComponent implements OnInit {
  state = signal<'loading' | 'error'>('loading');
  error = signal('');

  private readonly auth = inject(ErnoAuthService);
  private readonly alerts = inject(ErnoAlertsService);
  private readonly route = inject(ActivatedRoute);
  private readonly router = inject(Router);

  ngOnInit() {
    const token = this.route.snapshot.queryParamMap.get('token') ?? '';
    if (!token) {
      this.state.set('error');
      this.error.set('Invalid or missing verification token.');
      return;
    }
    this.auth.verifyEmail(token).subscribe({
      next: () => {
        this.alerts.success('Email verified!');
        this.router.navigate(['/']);
      },
      error: (e) => {
        this.state.set('error');
        this.error.set(e?.error?.message ?? 'Verification failed. The link may have expired.');
      },
    });
  }
}
