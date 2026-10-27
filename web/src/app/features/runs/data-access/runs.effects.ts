import { inject } from '@angular/core';
import { MatSnackBar } from '@angular/material/snack-bar';
import { Actions, createEffect, ofType } from '@ngrx/effects';
import { routerNavigatedAction } from '@ngrx/router-store';
import { Store } from '@ngrx/store';
import { catchError, filter, map, mergeMap, of, switchMap, tap, withLatestFrom } from 'rxjs';

import { errorMessage, StudioApi } from '../../../shared/api/studio-api';
import { JobEventsActions, JobsActions } from '../../jobs/data-access/jobs.actions';
import { RunsActions } from './runs.actions';
import { selectCurrentRunName } from './runs.reducer';

/** Ce qu'affiche la page courante est chargé à chaque navigation, et relu à la reconnexion du flux. */
export const loadForPage$ = createEffect(
  (actions$ = inject(Actions), store = inject(Store)) =>
    actions$.pipe(
      ofType(routerNavigatedAction, JobEventsActions.connected),
      withLatestFrom(store.select(selectCurrentRunName)),
      map(([, name]) => (name ? RunsActions.loadRun({ name }) : RunsActions.loadRuns())),
    ),
  { functional: true },
);

export const loadRuns$ = createEffect(
  (actions$ = inject(Actions), api = inject(StudioApi)) =>
    actions$.pipe(
      ofType(RunsActions.loadRuns),
      switchMap(() =>
        api.runs().pipe(
          map((runs) => RunsActions.loadRunsSuccess({ runs })),
          catchError((error) => of(RunsActions.loadRunsFailure({ error: errorMessage(error) }))),
        ),
      ),
    ),
  { functional: true },
);

export const loadRun$ = createEffect(
  (actions$ = inject(Actions), api = inject(StudioApi)) =>
    actions$.pipe(
      ofType(RunsActions.loadRun),
      mergeMap(({ name }) =>
        api.run(name).pipe(
          map((run) => RunsActions.loadRunSuccess({ run })),
          catchError((error) => of(RunsActions.loadRunFailure({ name, error: errorMessage(error) }))),
        ),
      ),
    ),
  { functional: true },
);

export const loadPublication$ = createEffect(
  (actions$ = inject(Actions), api = inject(StudioApi)) =>
    actions$.pipe(
      ofType(RunsActions.loadRunSuccess),
      filter(({ run }) => run.artifacts.includes('publication')),
      mergeMap(({ run }) =>
        api.text(run.name, 'publication').pipe(
          map((text) => RunsActions.loadPublicationSuccess({ name: run.name, text })),
          catchError(() => of()),
        ),
      ),
    ),
  { functional: true },
);

/** Toute erreur remonte en bas de l'écran. */
export const showErrors$ = createEffect(
  (actions$ = inject(Actions), snackBar = inject(MatSnackBar)) =>
    actions$.pipe(
      ofType(
        RunsActions.loadRunsFailure,
        RunsActions.loadRunFailure,
        JobsActions.enqueueJobFailure,
        JobsActions.cancelJobFailure,
      ),
      tap(({ error }) => snackBar.open(error, 'Fermer', { duration: 8000 })),
    ),
  { functional: true, dispatch: false },
);
