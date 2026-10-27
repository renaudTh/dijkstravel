import { describe, expect, it } from 'vitest';

import { Job, RunDetail } from '../../../shared/api/api.models';
import { runActions } from './run-actions';

function run(overrides: Partial<RunDetail> = {}): RunDetail {
  return {
    name: 'essai',
    title: null,
    profile: 'car',
    mode: 'loop',
    format: '9:16',
    theme: 'default',
    durationSec: 60,
    places: null,
    stops: null,
    meters: null,
    artifacts: [],
    latestJob: null,
    spec: {},
    files: [],
    jobs: [],
    ...overrides,
  };
}

const running = { status: 'running' } as Job;
const byId = (actions: ReturnType<typeof runActions>) => Object.fromEntries(actions.map((a) => [a.id, a]));

describe('runActions', () => {
  it('requires the output of the previous step', () => {
    const bare = byId(runActions(run(), null, { car: true, foot: false, train: false }));
    expect(bare['solve'].enabled).toBe(false);
    expect(bare['solve'].reason).toContain('lieux absents');
    expect(bare['render'].reason).toContain('tournée absente');

    const solved = byId(runActions(run({ artifacts: ['places', 'tour'] }), null, null));
    expect(solved['solve'].enabled).toBe(true);
    expect(solved['render'].enabled).toBe(true);
    expect(solved['preview'].kind).toEqual({ kind: 'render', seconds: 10 });
  });

  it('needs the overpass and publication sections', () => {
    const plain = byId(runActions(run({ artifacts: ['places', 'tour'] }), null, null));
    expect(plain['source'].enabled).toBe(false);
    expect(plain['publication'].enabled).toBe(false);
    const full = byId(
      runActions(run({ artifacts: ['tour'], spec: { overpass: {}, publication: {} } }), null, null),
    );
    expect(full['source'].enabled).toBe(true);
    expect(full['publication'].enabled).toBe(true);
  });

  it('greys solving when the profile routing is down', () => {
    const actions = byId(
      runActions(run({ profile: 'train', artifacts: ['places'] }), null, { car: true, foot: true, train: false }),
    );
    expect(actions['solve'].reason).toContain('routage train arrêté');
    expect(actions['all'].enabled).toBe(false);
  });

  it('blocks everything while a job runs', () => {
    const actions = runActions(run({ artifacts: ['places', 'tour'], spec: { overpass: {} } }), running, null);
    expect(actions.every((action) => !action.enabled)).toBe(true);
    expect(actions[0].reason).toContain('déjà en cours');
  });
});
