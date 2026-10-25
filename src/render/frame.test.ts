import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import type { Tour } from '../solve/tour.ts';
import { frameCount, hudAt, momentAt, timeline } from './frame.ts';
import type { Timeline } from './frame.ts';

// Départ en coordonnées : le premier lieu est atteint après 0 m.
const TOUR: Tour = {
  run: 'frame-test',
  mode: 'open',
  start: [-4, 48],
  path: {
    points: [
      [-4, 48],
      [-3, 48],
      [-2, 48],
      [-1, 48],
    ],
    meters: [0, 1000, 2000, 3000],
  },
  stops: [
    { id: 'a', name: 'Alpha', at: [-3, 48], meters: 1000 },
    { id: 'b', name: 'Bravo', at: [-2, 48], meters: 2000 },
    { id: 'c', name: 'Charlie', at: [-1, 48], meters: 3000 },
  ],
  excluded: [],
  ferries: [],
  routedMeters: 3000,
};
const LENGTH = 3000;

const TIMELINE: Timeline = { intro: 90, travel: 300, outro: 90 };

function travelFrames(): number[] {
  return Array.from({ length: TIMELINE.travel }, (_, i) => TIMELINE.intro + i);
}

function travelHuds(): ReturnType<typeof hudAt>[] {
  return travelFrames().flatMap((frame) => {
    const moment = momentAt(TIMELINE, TOUR, frame);
    return moment.phase === 'travel' ? [hudAt(TOUR, moment.meters)] : [];
  });
}

function decreases(values: readonly number[]): number[] {
  return values.flatMap((value, i) => {
    const previous = values[i - 1];
    return previous !== undefined && value < previous ? [i] : [];
  });
}

describe('timeline', () => {
  test('counts intro, travel and outro frames at the run fps', () => {
    const video = { format: '16:9', durationSec: 10, introSec: 3, outroSec: 2, fps: 30 } as const;

    assert.equal(frameCount(timeline(video)), 450);
  });

  test('counts no intro frame when the intro lasts 0 s', () => {
    const video = { format: '16:9', durationSec: 10, introSec: 0, outroSec: 3, fps: 30 } as const;

    assert.equal(timeline(video).intro, 0);
  });
});

describe('momentAt and hudAt', () => {
  test('starts the travel at 0 m on its first frame', () => {
    assert.deepEqual(momentAt(TIMELINE, TOUR, TIMELINE.intro), { phase: 'travel', meters: 0 });
  });

  test('shows 0 m and no visited place at 0 m when the start is coordinates', () => {
    const hud = hudAt(TOUR, 0);

    assert.deepEqual(
      { meters: hud.meters, visited: hud.visited, total: hud.total },
      { meters: 0, visited: 0, total: 3 },
    );
  });

  test('ends the travel at the full length on its last frame', () => {
    assert.deepEqual(momentAt(TIMELINE, TOUR, TIMELINE.intro + TIMELINE.travel - 1), {
      phase: 'travel',
      meters: LENGTH,
    });
  });

  test('shows the total distance at the full length', () => {
    assert.equal(hudAt(TOUR, LENGTH).meters, LENGTH);
  });

  test('shows every place visited at the full length', () => {
    const hud = hudAt(TOUR, LENGTH);

    assert.deepEqual({ visited: hud.visited, total: hud.total }, { visited: 3, total: 3 });
  });

  test('marks a place visited when the head reaches its distance', () => {
    assert.equal(hudAt(TOUR, 2000).visited, 2);
  });

  test('does not mark a place visited just before its distance', () => {
    assert.equal(hudAt(TOUR, 1999.9).visited, 1);
  });

  test('shows the last reached place name', () => {
    assert.equal(hudAt(TOUR, 2000).lastStop, 'Bravo');
  });

  test('never decreases the distance from one frame to the next', () => {
    assert.deepEqual(decreases(travelHuds().map((hud) => hud.meters)), []);
  });

  test('never decreases the visited count from one frame to the next', () => {
    assert.deepEqual(decreases(travelHuds().map((hud) => hud.visited)), []);
  });
});

describe('momentAt with a slowdown near places', () => {
  const SLOWDOWN = 2;

  function travelMeters(): number[] {
    return travelFrames().flatMap((frame) => {
      const moment = momentAt(TIMELINE, TOUR, frame, SLOWDOWN);
      return moment.phase === 'travel' ? [moment.meters] : [];
    });
  }

  function stepAcross(meters: readonly number[], at: number): number {
    const i = meters.findIndex((value, index) => value <= at && (meters[index + 1] ?? -Infinity) > at);
    return (meters[i + 1] ?? NaN) - (meters[i] ?? NaN);
  }

  test('moves slower when passing a place than halfway between two places', () => {
    const meters = travelMeters();

    assert.ok(stepAcross(meters, 1000) < stepAcross(meters, 1500));
  });

  test('still reaches the full length on the last travel frame', () => {
    assert.equal(travelMeters().at(-1), LENGTH);
  });

  test('never goes backwards', () => {
    assert.deepEqual(decreases(travelMeters()), []);
  });
});
