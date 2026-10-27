import { createActionGroup, emptyProps, props } from '@ngrx/store';

import { RunDetail, RunSummary } from '../../../shared/api/api.models';

export const RunsActions = createActionGroup({
  source: 'Runs',
  events: {
    'Load Runs': emptyProps(),
    'Load Runs Success': props<{ runs: RunSummary[] }>(),
    'Load Runs Failure': props<{ error: string }>(),
    'Load Run': props<{ name: string }>(),
    'Load Run Success': props<{ run: RunDetail }>(),
    'Load Run Failure': props<{ name: string; error: string }>(),
    'Load Publication Success': props<{ name: string; text: string }>(),
  },
});
