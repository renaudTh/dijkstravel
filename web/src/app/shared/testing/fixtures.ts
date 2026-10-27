import { Job } from '../api/api.models';

/** Un job d'essai, à compléter. */
export function job(overrides: Partial<Job> = {}): Job {
  return {
    id: 'j1',
    runId: 'r1',
    runName: 'essai',
    kind: { kind: 'solve' },
    status: 'queued',
    progress: null,
    log: [],
    error: null,
    cancelRequested: false,
    createdAt: '2026-10-27T10:00:00Z',
    startedAt: null,
    finishedAt: null,
    ...overrides,
  };
}
