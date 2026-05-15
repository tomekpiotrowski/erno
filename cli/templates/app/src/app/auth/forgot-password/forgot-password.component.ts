import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { FormControl, FormGroup, Validators } from '@angular/forms';
import { ErnoAuthService } from 'erno-angular';

@Component({
  selector: 'app-forgot-password',
  templateUrl: './forgot-password.component.html',
  standalone: false,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ForgotPasswordComponent {
  form = new FormGroup({
    email: new FormControl('', [Validators.required, Validators.email]),
  });
  loading = signal(false);
  done = signal(false);

  constructor(private auth: ErnoAuthService) {}

  submit() {
    if (this.form.invalid || this.loading()) return;
    this.loading.set(true);
    this.auth.requestPasswordReset(this.form.value.email!).subscribe({
      next: () => { this.done.set(true); },
      error: () => { this.done.set(true); },
    });
  }
}
