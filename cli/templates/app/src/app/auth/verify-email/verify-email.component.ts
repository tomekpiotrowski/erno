import { Component, OnInit } from '@angular/core';
import { ActivatedRoute } from '@angular/router';
import { ErnoAuthService } from 'erno-angular';

@Component({
  selector: 'app-verify-email',
  templateUrl: './verify-email.component.html',
  standalone: false,
})
export class VerifyEmailComponent implements OnInit {
  state: 'loading' | 'success' | 'error' = 'loading';
  error = '';

  constructor(private auth: ErnoAuthService, private route: ActivatedRoute) {}

  ngOnInit() {
    const token = this.route.snapshot.queryParamMap.get('token') ?? '';
    if (!token) {
      this.state = 'error';
      this.error = 'Invalid or missing verification token.';
      return;
    }
    this.auth.verifyEmail(token).subscribe({
      next: () => { this.state = 'success'; },
      error: (e) => {
        this.state = 'error';
        this.error = e?.error?.message ?? 'Verification failed. The link may have expired.';
      },
    });
  }
}
