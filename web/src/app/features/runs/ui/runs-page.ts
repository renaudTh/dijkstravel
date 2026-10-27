import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatProgressSpinnerModule } from '@angular/material/progress-spinner';
import { Store } from '@ngrx/store';

import { runsFeature } from '../data-access/runs.reducer';
import { RunCard } from './run-card';

@Component({
  selector: 'app-runs-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatProgressSpinnerModule, RunCard],
  template: `
    <h1>Runs</h1>
    @if (!loaded()) {
      @if (error(); as error) {
        <p class="error">Liste des runs indisponible : {{ error }}</p>
      } @else {
        <mat-spinner diameter="40" />
      }
    } @else if (runs().length === 0) {
      <p>Aucun run. Ils apparaissent ici après l’import du dépôt (<code>IMPORT_RUNS_DIR</code>).</p>
    } @else {
      <ul class="grid">
        @for (run of runs(); track run.name) {
          <li><app-run-card [run]="run" /></li>
        }
      </ul>
    }
  `,
  styles: `
    h1 { font: var(--mat-sys-headline-medium); margin: 0 0 16px; }
    .grid { list-style: none; margin: 0; padding: 0; display: grid; gap: 16px; grid-template-columns: repeat(auto-fill, minmax(260px, 1fr)); }
    .error { color: var(--mat-sys-error); }
  `,
})
export class RunsPage {
  private readonly store = inject(Store);
  protected readonly runs = this.store.selectSignal(runsFeature.selectAllRuns);
  protected readonly loaded = this.store.selectSignal(runsFeature.selectLoaded);
  protected readonly error = this.store.selectSignal(runsFeature.selectError);
}
