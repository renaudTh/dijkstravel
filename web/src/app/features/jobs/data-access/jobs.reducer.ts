import { EntityState, createEntityAdapter } from '@ngrx/entity';
import { createFeature, createReducer, createSelector, on } from '@ngrx/store';

import { Job, isFinished } from '../../../shared/api/api.models';
import { RunsActions } from '../../runs/data-access/runs.actions';
import { JobEventsActions, JobsActions } from './jobs.actions';

export interface JobsState extends EntityState<Job> {
  /** Le flux SSE est-il ouvert ? Sans lui, l'avancement n'arrive plus. */
  live: boolean;
}

const byNewest = (a: Job, b: Job) => b.createdAt.localeCompare(a.createdAt);
const adapter = createEntityAdapter<Job>({ sortComparer: byNewest });

export const jobsFeature = createFeature({
  name: 'jobs',
  reducer: createReducer<JobsState>(
    adapter.getInitialState({ live: false }),
    on(JobsActions.enqueueJobSuccess, JobsActions.cancelJobSuccess, JobEventsActions.jobChanged, (state, { job }) =>
      adapter.upsertOne(job, state),
    ),
    on(RunsActions.loadRunSuccess, (state, { run }) => adapter.upsertMany(run.jobs, state)),
    on(JobEventsActions.connected, (state) => ({ ...state, live: true })),
    on(JobEventsActions.disconnected, (state) => ({ ...state, live: false })),
  ),
  extraSelectors: ({ selectJobsState }) => {
    const { selectAll } = adapter.getSelectors(selectJobsState);
    return { selectAllJobs: selectAll };
  },
});

/** Les jobs d'un run, du plus récent au plus ancien. */
export const selectJobsOfRun = (runName: string) =>
  createSelector(jobsFeature.selectAllJobs, (jobs) => jobs.filter((job) => job.runName === runName));

/** Le job en file ou en cours sur ce run, s'il y en a un. */
export const selectActiveJobOfRun = (runName: string) =>
  createSelector(selectJobsOfRun(runName), (jobs) => jobs.find((job) => !isFinished(job)) ?? null);
