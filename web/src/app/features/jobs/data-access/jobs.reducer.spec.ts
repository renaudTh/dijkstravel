import { describe, expect, it } from 'vitest';

import { job } from '../../../shared/testing/fixtures';
import { runsFeature } from '../../runs/data-access/runs.reducer';
import { RunsActions } from '../../runs/data-access/runs.actions';
import { JobEventsActions, JobsActions } from './jobs.actions';
import { jobsFeature, selectActiveJobOfRun, selectJobsOfRun } from './jobs.reducer';

const reduce = (...actions: Parameters<typeof jobsFeature.reducer>[1][]) =>
  actions.reduce((state, action) => jobsFeature.reducer(state, action), jobsFeature.reducer(undefined, { type: '@init' }));

describe('jobs reducer', () => {
  it('follows a job from its creation to its end through events', () => {
    const state = reduce(
      JobsActions.enqueueJobSuccess({ job: job() }),
      JobEventsActions.jobChanged({
        job: job({ status: 'running', progress: { step: 'render', stage: 'frames', done: 30, total: 300 } }),
      }),
      JobEventsActions.jobChanged({ job: job({ status: 'succeeded' }) }),
    );
    const root = { jobs: state };
    expect(selectJobsOfRun('essai')(root)).toHaveLength(1);
    expect(selectJobsOfRun('essai')(root)[0].status).toBe('succeeded');
    expect(selectActiveJobOfRun('essai')(root)).toBeNull();
  });

  it('finds the active job among the run jobs, newest first', () => {
    const state = reduce(
      JobEventsActions.jobChanged({ job: job({ id: 'old', status: 'failed', createdAt: '2026-10-27T09:00:00Z' }) }),
      JobEventsActions.jobChanged({ job: job({ id: 'new', status: 'running' }) }),
      JobEventsActions.jobChanged({ job: job({ id: 'other', runName: 'autre', status: 'running' }) }),
    );
    const root = { jobs: state };
    expect(selectJobsOfRun('essai')(root).map((j) => j.id)).toEqual(['new', 'old']);
    expect(selectActiveJobOfRun('essai')(root)?.id).toBe('new');
  });

  it('tracks whether the live stream is open', () => {
    expect(reduce(JobEventsActions.connected()).live).toBe(true);
    expect(reduce(JobEventsActions.connected(), JobEventsActions.disconnected()).live).toBe(false);
  });
});

describe('runs reducer', () => {
  it('keeps the newest job on the run summary', () => {
    const summary = {
      name: 'essai',
      title: null,
      profile: 'car' as const,
      mode: 'loop' as const,
      format: '9:16' as const,
      theme: 'default',
      durationSec: 60,
      places: 3,
      stops: null,
      meters: null,
      artifacts: [],
      latestJob: null,
    };
    let state = runsFeature.reducer(undefined, RunsActions.loadRunsSuccess({ runs: [summary] }));
    state = runsFeature.reducer(state, JobEventsActions.jobChanged({ job: job({ id: 'new', status: 'running' }) }));
    state = runsFeature.reducer(
      state,
      JobEventsActions.jobChanged({ job: job({ id: 'late', status: 'failed', createdAt: '2026-10-27T08:00:00Z' }) }),
    );
    expect(state.entities['essai']?.latestJob?.id).toBe('new');
  });
});
