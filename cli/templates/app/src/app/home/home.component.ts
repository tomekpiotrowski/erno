import { Component } from '@angular/core';
import { Router } from '@angular/router';
import { ErnoAuthService } from 'erno-angular';

@Component({
  selector: 'app-home',
  templateUrl: './home.component.html',
  standalone: false,
})
export class HomeComponent {
  constructor(public auth: ErnoAuthService, private router: Router) {}

  logout() {
    this.auth.logout().subscribe({
      next: () => this.router.navigate(['/login']),
      error: () => this.router.navigate(['/login']),
    });
  }
}
