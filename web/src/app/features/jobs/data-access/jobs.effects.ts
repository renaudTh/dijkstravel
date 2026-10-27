import { inject } from '@angular/core';
import { Actions, ROOT_EFFECTS_INIT, createEffect, ofType } from '@ngrx/effects';
import { catchError, defer, filter, map, mergeMap, of, repeat, switchMap, timer } from 'rxjs';

import { errorMessage, StudioApi } from '../../../shared/api/studio-api';
import { RunsActions } from '../../runs/data-access/runs.actions';
import { JobStream, RECONNECT_MS } from './job-stream';
import { JobEventsActions, JobsActions } from './jobs.actions';

export const enqueueJob$ = createEffect(
  (actions$ = inject(Actions), api = inject(StudioApi)) =>
    actions$.pipe(
      ofType(JobsActions.enqueueJob),
      mergeMap(({ runName, kind }) =>
        api.enqueue(runName, kind).pipe(
          map((job) => JobsActions.enqueueJobSuccess({ job })),
          catchError((error) => of(JobsActions.enqueueJobFailure({ error: errorMessage(error) }))),
        ),
      ),
    ),
  { functional: true },
);

export const cancelJob$ = createEffect(
  (actions$ = inject(Actions), api = inject(StudioApi)) =>
    actions$.pipe(
      ofType(JobsActions.cancelJob),
      mergeMap(({ id }) =>
        api.cancel(id).pipe(
          map((job) => JobsActions.cancelJobSuccess({ job })),
          catchError((error) => of(JobsActions.cancelJobFailure({ error: errorMessage(error) }))),
        ),
      ),
    ),
  { functional: true },
);

/** Le flux SSE, ouvert au démarrage et rouvert après chaque coupure. */
export const jobEvents$ = createEffect(
  (actions$ = inject(Actions), stream = inject(JobStream)) =>
    actions$.pipe(
      ofType(ROOT_EFFECTS_INIT),
      switchMap(() =>
        defer(() => stream.connect()).pipe(
          map((event) =>
            event.type === 'open' ? JobEventsActions.connected() : JobEventsActions.jobChanged({ job: event.job }),
          ),
          catchError(() => of(JobEventsActions.disconnected())),
          repeat({ delay: () => timer(RECONNECT_MS) }),
        ),
      ),
    ),
  { functional: true },
);

/** Un job fini a pu produire des fichiers : le run est relu. */
export const reloadRunWhenJobEnds$ = createEffect(
  (actions$ = inject(Actions)) =>
    actions$.pipe(
      ofType(JobEventsActions.jobChanged, JobsActions.cancelJobSuccess),
      filter(({ job }) => job.status === 'succeeded' || job.status === 'failed' || job.status === 'cancelled'),
      map(({ job }) => RunsActions.loadRun({ name: job.runName })),
    ),
  { functional: true },
);
