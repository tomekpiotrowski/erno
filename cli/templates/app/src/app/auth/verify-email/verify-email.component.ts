import { ChangeDetectionStrategy, Component, OnInit, signal } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { ErnoAuthService } from 'erno-angular';

@Component({
  selector: 'app-verify-email',
  templateUrl: './verify-email.component.html',
  standalone: false,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class VerifyEmailComponent implements OnInit {
  state = signal<'loading' | 'success' | 'error'>('loading');
  error = signal('');

  constructor(private auth: ErnoAuthService, private route: ActivatedRoute) {}

  ngOnInit() {
    const token = this.route.snapshot.queryParamMap.get('token') ?? '';
    if (!token) {
      this.state.set('error');
      this.error.set('Invalid or missing verification token.');
      return;
    }
    this.auth.verifyEmail(token).subscribe({
      next: () => { this.state.set('success'); },
      error: (e) => {
        this.state.set('error');
        this.error.set(e?.error?.message ?? 'Verification failed. The link may have expired.');
      },
    });
  }
}
