import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormGroup, FormControl, ReactiveFormsModule, Validators } from '@angular/forms';
import { RouterLink } from '@angular/router';
import {
  IonButton,
  IonCard,
  IonCardContent,
  IonCardHeader,
  IonCardTitle,
  IonContent,
  IonInput,
  IonItem,
  IonLabel,
  IonText,
} from '@ionic/angular';
import { ErnoAuthService } from 'erno-angular';

@Component({
  selector: 'app-login',
  templateUrl: './login.component.html',
  imports: [
    ReactiveFormsModule,
    RouterLink,
    IonContent,
    IonCard,
    IonCardHeader,
    IonCardTitle,
    IonCardContent,
    IonItem,
    IonLabel,
    IonInput,
    IonText,
    IonButton,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LoginComponent {
  form = new FormGroup({
    email: new FormControl('', [Validators.required, Validators.email]),
    password: new FormControl('', Validators.required),
  });
  error = signal('');
  loading = signal(false);

  private readonly auth = inject(ErnoAuthService);

  submit() {
    if (this.form.invalid || this.loading()) return;
    this.loading.set(true);
    this.error.set('');
    const { email, password } = this.form.value;
    // No navigate here: `AppComponent` moves the app when the auth state does,
    // so signing in from anywhere — including the devtools panel — lands the
    // same way.
    this.auth.login(email!, password!).subscribe({
      error: (e) => {
        this.error.set(e?.error?.message ?? 'Invalid email or password');
        this.loading.set(false);
      },
    });
  }
}
