import { inject } from '@angular/core';
import { Actions, ROOT_EFFECTS_INIT, createEffect, ofType } from '@ngrx/effects';
import { createActionGroup, createFeature, createReducer, emptyProps, on, props } from '@ngrx/store';
import { catchError, map, of, switchMap, timer } from 'rxjs';

import { RoutingStatus } from '../../../shared/api/api.models';
import { StudioApi } from '../../../shared/api/studio-api';

/** Un OSRM démarré ou arrêté (profil compose) se voit en moins de 30 s. */
export const ROUTING_POLL_MS = 30_000;

export const RoutingActions = createActionGroup({
  source: 'Routing',
  events: {
    'Load Status Success': props<{ status: RoutingStatus }>(),
    'Load Status Failure': emptyProps(),
  },
});

export const routingFeature = createFeature({
  name: 'routing',
  reducer: createReducer<{ status: RoutingStatus | null }>(
    { status: null },
    on(RoutingActions.loadStatusSuccess, (_, { status }) => ({ status })),
    on(RoutingActions.loadStatusFailure, () => ({ status: null })),
  ),
});

export const pollRouting$ = createEffect(
  (actions$ = inject(Actions), api = inject(StudioApi)) =>
    actions$.pipe(
      ofType(ROOT_EFFECTS_INIT),
      switchMap(() => timer(0, ROUTING_POLL_MS)),
      switchMap(() =>
        api.routing().pipe(
          map((status) => RoutingActions.loadStatusSuccess({ status })),
          catchError(() => of(RoutingActions.loadStatusFailure())),
        ),
      ),
    ),
  { functional: true },
);
