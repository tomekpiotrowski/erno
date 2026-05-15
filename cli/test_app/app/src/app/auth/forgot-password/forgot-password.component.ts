import { Component } from '@angular/core';
import { FormControl, FormGroup, Validators } from '@angular/forms';
import { ErnoAuthService } from 'erno-angular';

@Component({
  selector: 'app-forgot-password',
  templateUrl: './forgot-password.component.html',
  standalone: false,
})
export class ForgotPasswordComponent {
  form = new FormGroup({
    email: new FormControl('', [Validators.required, Validators.email]),
  });
  loading = false;
  done = false;

  constructor(private auth: ErnoAuthService) {}

  submit() {
    if (this.form.invalid || this.loading) return;
    this.loading = true;
    this.auth.requestPasswordReset(this.form.value.email!).subscribe({
      next: () => { this.done = true; },
      error: () => { this.done = true; },
    });
  }
}
