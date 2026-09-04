import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import {
  AlertController,
  IonButton,
  IonButtons,
  IonContent,
  IonHeader,
  IonTitle,
  IonToolbar,
} from '@ionic/angular';
import { ErnoAuthService, ErnoAlertsService } from 'erno-angular';

@Component({
  selector: 'app-home',
  templateUrl: './home.page.html',
  imports: [IonHeader, IonToolbar, IonTitle, IonButtons, IonButton, IonContent],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class HomePage {
  readonly auth = inject(ErnoAuthService);
  private readonly alertController = inject(AlertController);
  private readonly alerts = inject(ErnoAlertsService);

  logout() {
    // `logout()` clears the session either way; `AppComponent` follows it to
    // the login page.
    this.auth.logout().subscribe({ error: () => undefined });
  }

  async confirmDeleteAccount() {
    const alert = await this.alertController.create({
      header: 'Delete account',
      message: 'This permanently deletes your account and all your data. Any active subscription is cancelled immediately. This cannot be undone. Enter your password to confirm.',
      inputs: [{ name: 'password', type: 'password', placeholder: 'Current password' }],
      buttons: [
        { text: 'Cancel', role: 'cancel' },
        {
          text: 'Delete',
          role: 'destructive',
          handler: (data) => {
            this.deleteAccount(data.password);
          },
        },
      ],
    });
    await alert.present();
  }

  private deleteAccount(password: string) {
    this.auth.deleteAccount(password).subscribe({
      error: (err) =>
        this.alerts.error(err?.status === 403 ? 'Incorrect password.' : 'Could not delete account.'),
    });
  }
}
