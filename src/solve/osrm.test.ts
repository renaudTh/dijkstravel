import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { ferrySegments, OsrmError } from './osrm.ts';

/** Step OSRM de `points` coordonnées (géométrie factice : seul leur nombre compte). */
const step = (mode: string, points: number, type = 'turn') => ({
  mode,
  maneuver: { type },
  geometry: { coordinates: Array.from({ length: points }, (_, i) => [i, 0]) },
});

describe('ferrySegments', () => {
  test('finds no crossing on a road-only route', () => {
    const legs = [{ steps: [step('driving', 4, 'depart'), step('driving', 3), step('driving', 2, 'arrive')] }];

    assert.deepEqual(ferrySegments(legs, 5), []);
  });

  test('locates a ferry step by the segments of the steps before it', () => {
    const legs = [
      { steps: [step('driving', 4, 'depart'), step('ferry', 3), step('driving', 2), step('driving', 2, 'arrive')] },
    ];

    assert.deepEqual(ferrySegments(legs, 6), [[3, 5]]);
  });

  test('merges consecutive ferry steps into one crossing', () => {
    const legs = [
      { steps: [step('driving', 2, 'depart'), step('ferry', 3), step('ferry', 4), step('driving', 2, 'arrive')] },
    ];

    assert.deepEqual(ferrySegments(legs, 6), [[1, 6]]);
  });

  test('counts no segment for the arrival of each leg', () => {
    const legs = [
      { steps: [step('driving', 3, 'depart'), step('driving', 2, 'arrive')] },
      { steps: [step('ferry', 4, 'depart'), step('driving', 2, 'arrive')] },
    ];

    assert.deepEqual(ferrySegments(legs, 5), [[2, 5]]);
  });

  test('finds no crossing when OSRM sends no steps', () => {
    assert.deepEqual(ferrySegments([{ annotation: { distance: [1, 2] } }], 2), []);
  });

  test('rejects steps that do not cover the route', () => {
    const legs = [{ steps: [step('driving', 3, 'depart'), step('driving', 2, 'arrive')] }];

    assert.throws(() => ferrySegments(legs, 4), OsrmError);
  });
});
