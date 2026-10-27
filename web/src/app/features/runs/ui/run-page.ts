import { Clipboard } from '@angular/cdk/clipboard';
import { DecimalPipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, inject, linkedSignal } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatButtonToggleModule } from '@angular/material/button-toggle';
import { MatCardModule } from '@angular/material/card';
import { MatIconModule } from '@angular/material/icon';
import { MatListModule } from '@angular/material/list';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { MatSnackBar } from '@angular/material/snack-bar';
import { MatTooltipModule } from '@angular/material/tooltip';
import { RouterLink } from '@angular/router';
import { Store } from '@ngrx/store';

import { Artifact, isFinished } from '../../../shared/api/api.models';
import { fileUrl } from '../../../shared/api/studio-api';
import { JobsActions } from '../../jobs/data-access/jobs.actions';
import { jobsFeature } from '../../jobs/data-access/jobs.reducer';
import { JobPanel } from '../../jobs/ui/job-panel';
import { routingFeature } from '../../routing/data-access/routing.state';
import { selectCurrentPublication, selectCurrentRun } from '../data-access/runs.reducer';
import { formatKm } from '../domain/format';
import { RunAction, runActions } from '../domain/run-actions';

const FILE_LABELS: Record<Artifact['kind'], string> = {
  places: 'Lieux (places.geojson)',
  tour: 'Tournée (tour.json)',
  video: 'Vidéo (video.mp4)',
  preview: 'Aperçu (preview.mp4)',
  cover: 'Couverture (cover.jpg)',
  publication: 'Légende (publication.txt)',
};

@Component({
  selector: 'app-run-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    DecimalPipe,
    JobPanel,
    MatButtonModule,
    MatButtonToggleModule,
    MatCardModule,
    MatIconModule,
    MatListModule,
    MatProgressSpinnerModule,
    MatTooltipModule,
    RouterLink,
  ],
  template: `
    <a mat-button routerLink="/runs"><mat-icon>arrow_back</mat-icon> Runs</a>
    @if (run(); as run) {
      <header class="head">
        <div>
          <h1>{{ run.title ?? run.name }}</h1>
          <p class="figures">
            <span>{{ run.stops ?? run.places ?? '–' }} lieux</span>
            <span>{{ km() }}</span>
            <span>{{ run.format }} · {{ run.durationSec | number }} s</span>
            <span>profil {{ run.profile }}</span>
            <span>thème {{ run.theme }}</span>
          </p>
        </div>
      </header>

      <nav class="actions" aria-label="Actions sur le run">
        @for (action of actions(); track action.id) {
          <span [matTooltip]="action.reason ?? ''" [matTooltipDisabled]="action.enabled">
            @if (action.id === 'all') {
              <button mat-flat-button type="button" [disabled]="!action.enabled" (click)="launch(action)">
                <mat-icon>{{ action.icon }}</mat-icon>
                {{ action.label }}
              </button>
            } @else {
              <button mat-stroked-button type="button" [disabled]="!action.enabled" (click)="launch(action)">
                <mat-icon>{{ action.icon }}</mat-icon>
                {{ action.label }}
              </button>
            }
          </span>
        }
      </nav>

      <div class="layout">
        <section class="media">
          @if (hasVideo() && hasPreview()) {
            <mat-button-toggle-group
              hideSingleSelectionIndicator
              aria-label="Vidéo affichée"
              [value]="shown()"
              (change)="shown.set($event.value)"
            >
              <mat-button-toggle value="video">Vidéo</mat-button-toggle>
              <mat-button-toggle value="preview">Aperçu</mat-button-toggle>
            </mat-button-toggle-group>
          }
          @if (videoUrl(); as src) {
            <video [src]="src" controls preload="metadata" [class.portrait]="run.format === '9:16'"></video>
          } @else {
            <div class="empty">
              <mat-icon>movie</mat-icon>
              <p>Pas encore de vidéo. « Rendre » la produit à partir de la tournée.</p>
            </div>
          }
        </section>

        <aside class="side">
          @if (currentJob(); as job) {
            <mat-card appearance="outlined">
              <mat-card-content>
                <app-job-panel [job]="job" (cancel)="cancel($event)" />
              </mat-card-content>
            </mat-card>
          }

          @if (publication(); as text) {
            <mat-card appearance="outlined">
              <mat-card-header>
                <mat-card-title>Légende</mat-card-title>
              </mat-card-header>
              <mat-card-content>
                <pre class="publication">{{ text }}</pre>
              </mat-card-content>
              <mat-card-actions>
                <button mat-button type="button" (click)="copy(text)"><mat-icon>content_copy</mat-icon> Copier</button>
              </mat-card-actions>
            </mat-card>
          }

          @if (coverUrl(); as src) {
            <mat-card appearance="outlined">
              <img class="cover" [src]="src" alt="Couverture : tracé complet, sans la carte de titre" />
            </mat-card>
          }

          <mat-card appearance="outlined">
            <mat-card-header><mat-card-title>Fichiers</mat-card-title></mat-card-header>
            <mat-nav-list>
              @for (file of run.files; track file.kind) {
                <a mat-list-item [href]="downloadUrl(file)" [attr.download]="''">
                  <mat-icon matListItemIcon>download</mat-icon>
                  <span matListItemTitle>{{ fileLabel(file) }}</span>
                  <span matListItemLine>{{ file.bytes / 1e6 | number: '1.1-1' }} Mo</span>
                </a>
              } @empty {
                <p class="none">Aucun fichier produit.</p>
              }
            </mat-nav-list>
          </mat-card>
        </aside>
      </div>

      @if (history().length) {
        <section class="history">
          <h2>Historique</h2>
          @for (job of history(); track job.id) {
            <mat-card appearance="outlined">
              <mat-card-content><app-job-panel [job]="job" (cancel)="cancel($event)" /></mat-card-content>
            </mat-card>
          }
        </section>
      }
    } @else {
      <mat-spinner diameter="40" />
    }
  `,
  styles: `
    :host { display: grid; gap: 16px; }
    :host > a { justify-self: start; }
    h1 { font: var(--mat-sys-headline-medium); margin: 0; text-wrap: balance; }
    h2 { font: var(--mat-sys-title-large); margin: 0; }
    .figures { display: flex; flex-wrap: wrap; gap: 16px; margin: 4px 0 0; color: var(--mat-sys-on-surface-variant); font-variant-numeric: tabular-nums; }
    .actions { display: flex; flex-wrap: wrap; gap: 8px; }
    .layout { display: grid; gap: 16px; grid-template-columns: minmax(0, 1fr) minmax(280px, 380px); align-items: start; }
    @media (max-width: 900px) { .layout { grid-template-columns: minmax(0, 1fr); } }
    .media { display: grid; gap: 8px; }
    .media mat-button-toggle-group { justify-self: center; }
    .media video { width: 100%; max-height: 78vh; background: #000; border-radius: 12px; display: block; }
    .media video.portrait { width: auto; max-width: 100%; margin-inline: auto; }
    .empty { display: grid; place-items: center; gap: 8px; padding: 48px 16px; border-radius: 12px; text-align: center;
      background: var(--mat-sys-surface-container); color: var(--mat-sys-on-surface-variant); }
    .side { display: grid; gap: 16px; min-width: 0; }
    .publication { margin: 0; white-space: pre-wrap; font: var(--mat-sys-body-medium); }
    .cover { display: block; width: 100%; }
    .none { padding: 0 16px; color: var(--mat-sys-on-surface-variant); }
    .history { display: grid; gap: 8px; }
  `,
})
export class RunPage {
  private readonly store = inject(Store);
  private readonly clipboard = inject(Clipboard);
  private readonly snackBar = inject(MatSnackBar);

  protected readonly run = this.store.selectSignal(selectCurrentRun);
  protected readonly publication = this.store.selectSignal(selectCurrentPublication);
  private readonly routing = this.store.selectSignal(routingFeature.selectStatus);
  private readonly allJobs = this.store.selectSignal(jobsFeature.selectAllJobs);

  private readonly jobs = computed(() => {
    const name = this.run()?.name;
    return this.allJobs().filter((job) => job.runName === name);
  });
  /** Le job en cours, sinon le dernier : son issue reste visible. */
  protected readonly currentJob = computed(() => this.jobs().find((job) => !isFinished(job)) ?? this.jobs()[0] ?? null);
  protected readonly history = computed(() => this.jobs().filter((job) => job !== this.currentJob()).slice(0, 10));
  protected readonly actions = computed(() => {
    const run = this.run();
    const active = this.jobs().find((job) => !isFinished(job)) ?? null;
    return run ? runActions(run, active, this.routing()) : [];
  });
  protected readonly km = computed(() => formatKm(this.run()?.meters ?? null));

  /** Version du fichier : change quand un job le remplace, pour ne pas relire l'ancien en cache. */
  private version(kind: Artifact['kind']): string | null {
    const file = this.run()?.files.find((candidate) => candidate.kind === kind);
    return file ? `${file.bytes}-${this.currentJob()?.finishedAt ?? ''}` : null;
  }

  protected readonly hasVideo = computed(() => this.version('video') !== null);
  protected readonly hasPreview = computed(() => this.version('preview') !== null);
  /** La vidéo complète par défaut, l'aperçu juste après en avoir rendu un ; à défaut, celle qui existe. */
  protected readonly shown = linkedSignal<'video' | 'preview'>(() => {
    const kind = this.currentJob()?.kind;
    const preview = kind?.kind === 'render' && kind.seconds != null;
    return (preview && this.hasPreview()) || !this.hasVideo() ? 'preview' : 'video';
  });
  protected readonly videoUrl = computed(() => {
    const kind = this.shown();
    const version = this.version(kind);
    return version ? fileUrl(this.run()!.name, kind, version) : null;
  });
  protected readonly coverUrl = computed(() => {
    const version = this.version('cover');
    return version ? fileUrl(this.run()!.name, 'cover', version) : null;
  });

  protected launch(action: RunAction): void {
    const run = this.run();
    if (run && action.enabled) this.store.dispatch(JobsActions.enqueueJob({ runName: run.name, kind: action.kind }));
  }

  protected cancel(id: string): void {
    this.store.dispatch(JobsActions.cancelJob({ id }));
  }

  protected copy(text: string): void {
    const copied = this.clipboard.copy(text);
    this.snackBar.open(copied ? 'Légende copiée' : 'Copie impossible : sélectionner le texte', undefined, {
      duration: 3000,
    });
  }

  protected fileLabel(file: Artifact): string {
    return FILE_LABELS[file.kind];
  }

  protected downloadUrl(file: Artifact): string {
    return fileUrl(this.run()!.name, file.kind);
  }
}
