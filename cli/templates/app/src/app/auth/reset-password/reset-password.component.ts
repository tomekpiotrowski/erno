import { ChangeDetectionStrategy, Component, OnInit, signal } from '@angular/core';
import { AbstractControl, FormControl, FormGroup, ValidationErrors, Validators } from '@angular/forms';
import { ActivatedRoute, Router } from '@angular/router';
import { ErnoAuthService, ErnoAlertsService } from 'erno-angular';

function passwordsMatch(group: AbstractControl): ValidationErrors | null {
  const pw = group.get('password')?.value;
  const confirm = group.get('confirm')?.value;
  return pw && confirm && pw !== confirm ? { mismatch: true } : null;
}

@Component({
  selector: 'app-reset-password',
  templateUrl: './reset-password.component.html',
  standalone: false,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ResetPasswordComponent implements OnInit {
  form = new FormGroup(
    {
      password: new FormControl('', [Validators.required, Validators.minLength(8)]),
      confirm: new FormControl('', Validators.required),
    },
    { validators: passwordsMatch },
  );
  error = signal('');
  loading = signal(false);
  private token = '';

  constructor(
    private auth: ErnoAuthService,
    private alerts: ErnoAlertsService,
    private router: Router,
    private route: ActivatedRoute,
  ) {}

  ngOnInit() {
    this.token = this.route.snapshot.queryParamMap.get('token') ?? '';
    if (!this.token) {
      this.error.set('Invalid or missing reset token.');
    }
  }

  submit() {
    if (this.form.invalid || this.loading() || !this.token) return;
    this.loading.set(true);
    this.error.set('');
    this.auth.confirmPasswordReset(this.token, this.form.value.password!).subscribe({
      next: () => {
        this.alerts.success('Password updated — you can now sign in.');
        this.router.navigate(['/login']);
      },
      error: (e) => {
        this.error.set(e?.error?.message ?? 'Reset failed. The link may have expired.');
        this.loading.set(false);
      },
    });
  }
}
