import { DatePipe } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { MatButtonModule } from '@angular/material/button';
import { MatExpansionModule } from '@angular/material/expansion';
import { MatIconModule } from '@angular/material/icon';
import { MatProgressBarModule } from '@angular/material/progress-bar';

import { Job, isFinished } from '../../../shared/api/api.models';
import { STATUS_LABELS, kindLabel, progressLabel, progressPercent } from '../../runs/domain/format';

/** Un job : son étape, son avancement, son log ; annulable tant qu'il n'est pas fini. */
@Component({
  selector: 'app-job-panel',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DatePipe, MatButtonModule, MatExpansionModule, MatIconModule, MatProgressBarModule],
  template: `
    <section class="job" [attr.data-status]="job().status">
      <header>
        <div class="title">
          <span class="kind">{{ kind() }}</span>
          <span class="status">{{ status() }}</span>
          <time>{{ job().createdAt | date: 'd MMM, HH:mm' }}</time>
        </div>
        @if (!finished()) {
          <button
            mat-stroked-button
            type="button"
            (click)="cancel.emit(job().id)"
            [disabled]="job().cancelRequested"
          >
            <mat-icon>stop_circle</mat-icon>
            {{ job().cancelRequested ? 'Annulation…' : 'Annuler' }}
          </button>
        }
      </header>

      @if (!finished()) {
        <mat-progress-bar [mode]="percent() === null ? 'indeterminate' : 'determinate'" [value]="percent() ?? 0" />
        <p class="step">{{ step() }}</p>
      }
      @if (job().error; as error) {
        <p class="error">{{ error }}</p>
      }
      @if (job().log.length) {
        <mat-expansion-panel class="log">
          <mat-expansion-panel-header>
            <mat-panel-title>Journal ({{ job().log.length }})</mat-panel-title>
          </mat-expansion-panel-header>
          <pre>{{ job().log.join('\n') }}</pre>
        </mat-expansion-panel>
      }
    </section>
  `,
  styles: `
    .job { display: grid; gap: 8px; }
    header { display: flex; align-items: center; justify-content: space-between; gap: 12px; flex-wrap: wrap; }
    .title { display: flex; align-items: baseline; gap: 12px; flex-wrap: wrap; }
    .kind { font: var(--mat-sys-title-medium); }
    .status { font: var(--mat-sys-label-large); color: var(--mat-sys-on-surface-variant); }
    [data-status='failed'] .status { color: var(--mat-sys-error); }
    [data-status='succeeded'] .status { color: var(--mat-sys-primary); }
    time { font: var(--mat-sys-body-small); color: var(--mat-sys-on-surface-variant); }
    .step { margin: 0; font: var(--mat-sys-body-medium); font-variant-numeric: tabular-nums; }
    .error { margin: 0; color: var(--mat-sys-error); white-space: pre-wrap; }
    pre { margin: 0; white-space: pre-wrap; font: var(--mat-sys-body-small); font-family: ui-monospace, monospace; }
  `,
})
export class JobPanel {
  readonly job = input.required<Job>();
  readonly cancel = output<string>();

  protected readonly finished = computed(() => isFinished(this.job()));
  protected readonly kind = computed(() => kindLabel(this.job().kind));
  protected readonly status = computed(() => STATUS_LABELS[this.job().status]);
  protected readonly percent = computed(() => progressPercent(this.job().progress));
  protected readonly step = computed(() => {
    const progress = this.job().progress;
    return progress ? progressLabel(progress) : 'En attente d’un worker';
  });
}
