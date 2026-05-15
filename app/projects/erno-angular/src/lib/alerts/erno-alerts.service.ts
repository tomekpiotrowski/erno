import { Injectable, OnDestroy } from '@angular/core';
import { ToastController } from '@ionic/angular';
import { Subject, Subscription, concatMap, from } from 'rxjs';

export type AlertType = 'success' | 'info' | 'warn' | 'error';

interface AlertItem {
  type: AlertType;
  message: string;
  duration: number;
}

const COLOR_MAP: Record<AlertType, string> = {
  success: 'success',
  info: 'primary',
  warn: 'warning',
  error: 'danger',
};

@Injectable()
export class ErnoAlertsService implements OnDestroy {
  private queue$ = new Subject<AlertItem>();
  private sub: Subscription;

  constructor(private toastCtrl: ToastController) {
    this.sub = this.queue$
      .pipe(concatMap(item => from(this.present(item))))
      .subscribe();
  }

  success(message: string, duration = 3000): void {
    this.queue$.next({ type: 'success', message, duration });
  }

  info(message: string, duration = 3000): void {
    this.queue$.next({ type: 'info', message, duration });
  }

  warn(message: string, duration = 4000): void {
    this.queue$.next({ type: 'warn', message, duration });
  }

  error(message: string, duration = 5000): void {
    this.queue$.next({ type: 'error', message, duration });
  }

  ngOnDestroy(): void {
    this.sub.unsubscribe();
    this.queue$.complete();
  }

  private async present(item: AlertItem): Promise<void> {
    const toast = await this.toastCtrl.create({
      message: item.message,
      duration: item.duration,
      color: COLOR_MAP[item.type],
      position: 'top',
    });
    await toast.present();
    await toast.onDidDismiss();
  }
}
