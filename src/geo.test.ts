import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { pathUntil, pointAt } from './geo.ts';
import type { LonLat, MeasuredPath } from './geo.ts';

// Le long d'un méridien : l'interpolation linéaire et géodésique coïncident.
const PATH: MeasuredPath = {
  points: [
    [2, 46],
    [2, 47],
    [2, 49],
  ],
  meters: [0, 1000, 3000],
};
const LENGTH = 3000;

function rounded(point: LonLat): number[] {
  return point.map((value) => Math.round(value * 1e9) / 1e9);
}

describe('pointAt', () => {
  test('returns the first point at 0 m', () => {
    assert.deepEqual(rounded(pointAt(PATH, 0)), [2, 46]);
  });

  test('returns the last point at the full length', () => {
    assert.deepEqual(rounded(pointAt(PATH, LENGTH)), [2, 49]);
  });

  test('interpolates halfway along a segment', () => {
    assert.deepEqual(rounded(pointAt(PATH, 2000)), [2, 48]);
  });

  test('clamps a negative distance to the first point', () => {
    assert.deepEqual(rounded(pointAt(PATH, -1000)), [2, 46]);
  });

  test('clamps a distance beyond the end to the last point', () => {
    assert.deepEqual(rounded(pointAt(PATH, LENGTH + 1000)), [2, 49]);
  });

  test('returns a valid point on a path with two identical consecutive points', () => {
    const path: MeasuredPath = {
      points: [
        [2, 46],
        [2, 46],
        [2, 47],
      ],
      meters: [0, 0, 1000],
    };

    assert.equal(pointAt(path, 0).some(Number.isNaN), false);
  });

  test('returns the only point of a single-point path', () => {
    const path: MeasuredPath = { points: [[2, 46]], meters: [0] };

    assert.deepEqual(pointAt(path, 500), [2, 46]);
  });
});

describe('pathUntil', () => {
  test('traces nothing at 0 m', () => {
    assert.deepEqual(pathUntil(PATH, 0), []);
  });

  test('traces the whole path at its full length', () => {
    assert.deepEqual(pathUntil(PATH, LENGTH).map(rounded), [
      [2, 46],
      [2, 47],
      [2, 49],
    ]);
  });
});
