import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatToolbarModule } from '@angular/material/toolbar';
import { MatTooltipModule } from '@angular/material/tooltip';
import { RouterLink, RouterOutlet } from '@angular/router';
import { Store } from '@ngrx/store';

import { jobsFeature } from './features/jobs/data-access/jobs.reducer';
import { routingFeature } from './features/routing/data-access/routing.state';
import { Profile } from './shared/api/api.models';

const PROFILES: { profile: Profile; icon: string; label: string }[] = [
  { profile: 'car', icon: 'directions_car', label: 'voiture' },
  { profile: 'foot', icon: 'directions_walk', label: 'piéton' },
  { profile: 'train', icon: 'train', label: 'train' },
];

@Component({
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatIconModule, MatToolbarModule, MatTooltipModule, RouterLink, RouterOutlet],
  template: `
    <mat-toolbar>
      <a class="brand" routerLink="/runs">
        <mat-icon>route</mat-icon>
        <span>Dijkstravel</span>
      </a>
      <span class="spacer"></span>
      <ul class="status" aria-label="Services">
        @for (service of services(); track service.profile) {
          <li [class.up]="service.up" [matTooltip]="'Routage ' + service.label + (service.up ? ' disponible' : ' arrêté')">
            <mat-icon>{{ service.icon }}</mat-icon>
          </li>
        }
        <li [class.up]="live()" [matTooltip]="live() ? 'Suivi en direct actif' : 'Suivi en direct coupé, reconnexion…'">
          <mat-icon>{{ live() ? 'sensors' : 'sensors_off' }}</mat-icon>
        </li>
      </ul>
    </mat-toolbar>
    <main>
      <router-outlet />
    </main>
  `,
  styles: `
    mat-toolbar { position: sticky; top: 0; z-index: 2; gap: 8px; background: var(--mat-sys-surface-container); }
    .brand { display: inline-flex; align-items: center; gap: 8px; color: inherit; text-decoration: none; font: var(--mat-sys-title-large); }
    .spacer { flex: 1; }
    .status { list-style: none; display: flex; gap: 12px; margin: 0; padding: 0; }
    .status li { display: grid; color: var(--mat-sys-outline); }
    .status li.up { color: var(--mat-sys-primary); }
    main { max-width: 1400px; margin: 0 auto; padding: 24px max(16px, env(safe-area-inset-left)); }
  `,
})
export class App {
  private readonly store = inject(Store);
  private readonly routing = this.store.selectSignal(routingFeature.selectStatus);
  protected readonly live = this.store.selectSignal(jobsFeature.selectLive);
  protected readonly services = computed(() => {
    const routing = this.routing();
    return PROFILES.map((service) => ({ ...service, up: routing?.[service.profile] ?? false }));
  });
}
