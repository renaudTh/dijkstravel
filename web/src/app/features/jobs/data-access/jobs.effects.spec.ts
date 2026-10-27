import { TestBed } from '@angular/core/testing';
import { Action } from '@ngrx/store';
import { ROOT_EFFECTS_INIT } from '@ngrx/effects';
import { provideMockActions } from '@ngrx/effects/testing';
import { Observable, Subject, from, throwError } from 'rxjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { RunsActions } from '../../runs/data-access/runs.actions';
import { JobStream, JobStreamEvent, RECONNECT_MS } from './job-stream';
import { JobEventsActions } from './jobs.actions';
import { jobEvents$, reloadRunWhenJobEnds$ } from './jobs.effects';
import { job } from '../../../shared/testing/fixtures';

describe('job events effect', () => {
  let actions$: Subject<Action>;
  let connections: Observable<JobStreamEvent>[];

  beforeEach(() => {
    vi.useFakeTimers();
    actions$ = new Subject<Action>();
    connections = [];
    TestBed.configureTestingModule({
      providers: [
        provideMockActions(() => actions$),
        { provide: JobStream, useValue: { connect: () => connections.shift() ?? new Subject<JobStreamEvent>() } },
      ],
    });
  });

  afterEach(() => vi.useRealTimers());

  it('turns the stream into actions and reconnects after a cut', () => {
    connections.push(
      from<JobStreamEvent[]>([{ type: 'open' }, { type: 'job', job: job({ status: 'running' }) }]),
      throwError(() => new Error('coupé')),
      from<JobStreamEvent[]>([{ type: 'open' }]),
    );
    const seen: Action[] = [];
    TestBed.runInInjectionContext(() => jobEvents$()).subscribe((action) => seen.push(action));

    actions$.next({ type: ROOT_EFFECTS_INIT });
    // Le premier flux s'est terminé : on rouvre après le délai.
    vi.advanceTimersByTime(RECONNECT_MS);
    // Le deuxième coupe aussitôt : déconnexion signalée, puis nouvel essai.
    vi.advanceTimersByTime(RECONNECT_MS);

    expect(seen.map((action) => action.type)).toEqual([
      JobEventsActions.connected.type,
      JobEventsActions.jobChanged.type,
      JobEventsActions.disconnected.type,
      JobEventsActions.connected.type,
    ]);
  });

  it('reloads the run once its job has ended', () => {
    const seen: Action[] = [];
    TestBed.runInInjectionContext(() => reloadRunWhenJobEnds$()).subscribe((action) => seen.push(action));
    actions$.next(JobEventsActions.jobChanged({ job: job({ status: 'running' }) }));
    actions$.next(JobEventsActions.jobChanged({ job: job({ status: 'succeeded' }) }));
    expect(seen).toEqual([RunsActions.loadRun({ name: 'essai' })]);
  });
});
