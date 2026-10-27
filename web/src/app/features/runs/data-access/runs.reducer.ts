import { EntityState, createEntityAdapter } from '@ngrx/entity';
import { getRouterSelectors } from '@ngrx/router-store';
import { createFeature, createReducer, createSelector, on } from '@ngrx/store';

import { Job, RunDetail, RunSummary } from '../../../shared/api/api.models';
import { JobEventsActions, JobsActions } from '../../jobs/data-access/jobs.actions';
import { RunsActions } from './runs.actions';

export interface RunsState extends EntityState<RunSummary> {
  loaded: boolean;
  /** Détail des runs ouverts, par nom. */
  details: Record<string, RunDetail>;
  /** Texte de `publication.txt`, par nom de run. */
  publications: Record<string, string>;
  error: string | null;
}

const adapter = createEntityAdapter<RunSummary>({
  selectId: (run) => run.name,
  sortComparer: (a, b) => a.name.localeCompare(b.name),
});

/** Le dernier job connu d'un run : un événement plus ancien ne remplace pas un plus récent. */
function withJob(run: RunSummary, job: Job): RunSummary {
  const latest = run.latestJob;
  return !latest || latest.id === job.id || latest.createdAt <= job.createdAt ? { ...run, latestJob: job } : run;
}

function summaryOf({ spec: _spec, files: _files, jobs: _jobs, ...summary }: RunDetail): RunSummary {
  return summary;
}

export const runsFeature = createFeature({
  name: 'runs',
  reducer: createReducer<RunsState>(
    adapter.getInitialState({ loaded: false, details: {}, publications: {}, error: null }),
    on(RunsActions.loadRunsSuccess, (state, { runs }) =>
      adapter.setAll(runs, { ...state, loaded: true, error: null }),
    ),
    on(RunsActions.loadRunsFailure, (state, { error }) => ({ ...state, error })),
    on(RunsActions.loadRunSuccess, (state, { run }) =>
      adapter.upsertOne(summaryOf(run), { ...state, details: { ...state.details, [run.name]: run } }),
    ),
    on(RunsActions.loadPublicationSuccess, (state, { name, text }) => ({
      ...state,
      publications: { ...state.publications, [name]: text },
    })),
    on(JobEventsActions.jobChanged, JobsActions.enqueueJobSuccess, JobsActions.cancelJobSuccess, (state, { job }) => {
      const run = state.entities[job.runName];
      return run ? adapter.updateOne({ id: run.name, changes: withJob(run, job) }, state) : state;
    }),
  ),
  extraSelectors: ({ selectRunsState }) => ({ selectAllRuns: adapter.getSelectors(selectRunsState).selectAll }),
});

export const { selectRouteParam } = getRouterSelectors();
export const selectCurrentRunName = selectRouteParam('name');

export const selectCurrentRun = createSelector(
  runsFeature.selectDetails,
  selectCurrentRunName,
  (details, name) => (name ? (details[name] ?? null) : null),
);

export const selectCurrentPublication = createSelector(
  runsFeature.selectPublications,
  selectCurrentRunName,
  (publications, name) => (name ? (publications[name] ?? null) : null),
);
