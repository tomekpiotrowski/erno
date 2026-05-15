import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { AbstractControl, FormControl, FormGroup, ValidationErrors, Validators } from '@angular/forms';
import { ErnoAuthService } from 'erno-angular';

function passwordsMatch(group: AbstractControl): ValidationErrors | null {
  const pw = group.get('password')?.value;
  const confirm = group.get('confirm')?.value;
  return pw && confirm && pw !== confirm ? { mismatch: true } : null;
}

@Component({
  selector: 'app-register',
  templateUrl: './register.component.html',
  standalone: false,
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class RegisterComponent {
  form = new FormGroup(
    {
      email: new FormControl('', [Validators.required, Validators.email]),
      password: new FormControl('', [Validators.required, Validators.minLength(8)]),
      confirm: new FormControl('', Validators.required),
    },
    { validators: passwordsMatch },
  );
  error = signal('');
  loading = signal(false);
  done = signal(false);

  constructor(private auth: ErnoAuthService) {}

  submit() {
    if (this.form.invalid || this.loading()) return;
    this.loading.set(true);
    this.error.set('');
    const { email, password } = this.form.value;
    this.auth.register(email!, password!).subscribe({
      next: () => { this.done.set(true); },
      error: (e) => {
        this.error.set(e?.error?.message ?? 'Registration failed');
        this.loading.set(false);
      },
    });
  }
}
