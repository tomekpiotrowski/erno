import { Component } from '@angular/core';
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
  error = '';
  loading = false;
  done = false;

  constructor(private auth: ErnoAuthService) {}

  submit() {
    if (this.form.invalid || this.loading) return;
    this.loading = true;
    this.error = '';
    const { email, password } = this.form.value;
    this.auth.register(email!, password!).subscribe({
      next: () => { this.done = true; },
      error: (e) => {
        this.error = e?.error?.message ?? 'Registration failed';
        this.loading = false;
      },
    });
  }
}
