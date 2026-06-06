import { Component } from '@angular/core';
import { Router } from '@angular/router';
import { AlertController } from '@ionic/angular';
import { ErnoAuthService, ErnoAlertsService } from 'erno-angular';

@Component({
  selector: 'app-home',
  templateUrl: './home.page.html',
  standalone: false,
})
export class HomePage {
  constructor(
    public auth: ErnoAuthService,
    private router: Router,
    private alertController: AlertController,
    private alerts: ErnoAlertsService,
  ) {}

  logout() {
    this.auth.logout().subscribe({
      next: () => this.router.navigate(['/login']),
      error: () => this.router.navigate(['/login']),
    });
  }

  async confirmDeleteAccount() {
    const alert = await this.alertController.create({
      header: 'Delete account',
      message: 'This permanently deletes your account and all your data. This cannot be undone. Enter your password to confirm.',
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
      next: () => this.router.navigate(['/login']),
      error: (err) =>
        this.alerts.error(err?.status === 403 ? 'Incorrect password.' : 'Could not delete account.'),
    });
  }
}
