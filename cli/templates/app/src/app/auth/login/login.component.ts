import { Component } from '@angular/core';
import { FormGroup, FormControl, Validators } from '@angular/forms';
import { ActivatedRoute, Router } from '@angular/router';
import { ErnoAuthService } from 'erno-angular';

@Component({
  selector: 'app-login',
  templateUrl: './login.component.html',
  standalone: false,
})
export class LoginComponent {
  form = new FormGroup({
    email: new FormControl('', [Validators.required, Validators.email]),
    password: new FormControl('', Validators.required),
  });
  error = '';
  loading = false;
  resetSuccess = false;

  constructor(
    private auth: ErnoAuthService,
    private router: Router,
    private route: ActivatedRoute,
  ) {
    this.resetSuccess = this.route.snapshot.queryParamMap.get('reset') === '1';
  }

  submit() {
    if (this.form.invalid || this.loading) return;
    this.loading = true;
    this.error = '';
    const { email, password } = this.form.value;
    this.auth.login(email!, password!).subscribe({
      next: () => this.router.navigate(['/']),
      error: (e) => {
        this.error = e?.error?.message ?? 'Invalid email or password';
        this.loading = false;
      },
    });
  }
}
