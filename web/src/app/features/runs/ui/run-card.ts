import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatCardModule } from '@angular/material/card';
import { MatChipsModule } from '@angular/material/chips';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBarModule } from '@angular/material/progress-bar';
import { RouterLink } from '@angular/router';

import { RunSummary, isFinished } from '../../../shared/api/api.models';
import { fileUrl } from '../../../shared/api/studio-api';
import { STATUS_LABELS, formatKm, kindLabel, progressPercent } from '../domain/format';

const PROFILE_ICONS = { car: 'directions_car', foot: 'directions_walk', train: 'train' } as const;

@Component({
  selector: 'app-run-card',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatCardModule, MatChipsModule, MatIconModule, MatProgressBarModule, RouterLink],
  template: `
    <a class="card-link" [routerLink]="['/runs', run().name]">
      <mat-card appearance="outlined">
        <div class="cover" [class.portrait]="run().format === '9:16'">
          @if (cover(); as src) {
            <img [src]="src" [alt]="'Couverture de ' + run().name" loading="lazy" />
          } @else {
            <mat-icon class="placeholder">map</mat-icon>
          }
        </div>
        <mat-card-header>
          <mat-card-title>{{ run().title ?? run().name }}</mat-card-title>
          <mat-card-subtitle>{{ run().name }}</mat-card-subtitle>
        </mat-card-header>
        <mat-card-content>
          <p class="figures">
            <mat-icon inline>{{ profileIcon() }}</mat-icon>
            <span>{{ run().stops ?? run().places ?? '–' }} lieux</span>
            <span>{{ km() }}</span>
            <span>{{ run().format }}</span>
          </p>
          <mat-chip-set>
            @for (artifact of run().artifacts; track artifact) {
              <mat-chip disableRipple>{{ artifact }}</mat-chip>
            }
          </mat-chip-set>
        </mat-card-content>
        @if (job(); as job) {
          <mat-card-footer [attr.data-status]="job.status">
            <span>{{ jobLabel() }}</span>
            @if (running()) {
              <mat-progress-bar [mode]="percent() === null ? 'indeterminate' : 'determinate'" [value]="percent() ?? 0" />
            }
          </mat-card-footer>
        }
      </mat-card>
    </a>
  `,
  styles: `
    .card-link { display: block; height: 100%; color: inherit; text-decoration: none; border-radius: 12px; }
    .card-link:focus-visible { outline: 2px solid var(--mat-sys-primary); outline-offset: 2px; }
    mat-card { height: 100%; overflow: hidden; }
    .cover { aspect-ratio: 16 / 9; display: grid; place-items: center; background: var(--mat-sys-surface-container); }
    .cover img { width: 100%; height: 100%; object-fit: cover; }
    .placeholder { font-size: 48px; width: 48px; height: 48px; color: var(--mat-sys-outline); }
    .figures { display: flex; align-items: center; gap: 12px; flex-wrap: wrap; margin: 8px 0; font-variant-numeric: tabular-nums; }
    mat-card-footer { display: grid; gap: 4px; padding: 0 16px 12px; font: var(--mat-sys-label-medium); color: var(--mat-sys-on-surface-variant); }
    mat-card-footer[data-status='failed'] { color: var(--mat-sys-error); }
  `,
})
export class RunCard {
  readonly run = input.required<RunSummary>();

  protected readonly job = computed(() => this.run().latestJob);
  protected readonly running = computed(() => {
    const job = this.job();
    return job !== null && !isFinished(job);
  });
  protected readonly percent = computed(() => progressPercent(this.job()?.progress ?? null));
  protected readonly km = computed(() => formatKm(this.run().meters));
  protected readonly profileIcon = computed(() => PROFILE_ICONS[this.run().profile]);
  protected readonly cover = computed(() => {
    const run = this.run();
    return run.artifacts.includes('cover') ? fileUrl(run.name, 'cover', run.latestJob?.finishedAt ?? '') : null;
  });
  protected readonly jobLabel = computed(() => {
    const job = this.job();
    return job ? `${kindLabel(job.kind)} · ${STATUS_LABELS[job.status]}` : '';
  });
}
