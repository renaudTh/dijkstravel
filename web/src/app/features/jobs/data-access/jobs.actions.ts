import { createActionGroup, emptyProps, props } from '@ngrx/store';

import { Job, JobKind } from '../../../shared/api/api.models';

export const JobsActions = createActionGroup({
  source: 'Jobs',
  events: {
    'Enqueue Job': props<{ runName: string; kind: JobKind }>(),
    'Enqueue Job Success': props<{ job: Job }>(),
    'Enqueue Job Failure': props<{ error: string }>(),
    'Cancel Job': props<{ id: string }>(),
    'Cancel Job Success': props<{ job: Job }>(),
    'Cancel Job Failure': props<{ error: string }>(),
  },
});

/** Le flux SSE `/api/events`. */
export const JobEventsActions = createActionGroup({
  source: 'Job Events',
  events: {
    Connected: emptyProps(),
    Disconnected: emptyProps(),
    'Job Changed': props<{ job: Job }>(),
  },
});
