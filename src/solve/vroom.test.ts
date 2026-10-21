import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { buildVroomRequest, parseVroomSolution, VroomError } from './vroom.ts';

const THREE_PLACES = [
  [0, 5, 7],
  [5, 0, 4],
  [7, 4, 0],
];

// Trois lieux, puis un départ en coordonnées à l'indice 3.
const THREE_PLACES_AND_START = [
  [0, 5, 7, 2],
  [5, 0, 4, 3],
  [7, 4, 0, 6],
  [2, 3, 6, 0],
];

describe('buildVroomRequest', () => {
  test('creates one job per place', () => {
    const request = buildVroomRequest(THREE_PLACES, 3, { kind: 'index', index: 0 }, 'loop');

    assert.deepEqual(request.jobs, [
      { id: 0, location_index: 0 },
      { id: 1, location_index: 1 },
      { id: 2, location_index: 2 },
    ]);
  });

  test('creates a single job for a single place', () => {
    const matrix = [
      [0, 9],
      [9, 0],
    ];
    const request = buildVroomRequest(matrix, 1, { kind: 'index', index: 1 }, 'loop');

    assert.deepEqual(request.jobs, [{ id: 0, location_index: 0 }]);
  });

  test('ends the vehicle at its departure in loop mode', () => {
    const request = buildVroomRequest(THREE_PLACES_AND_START, 3, { kind: 'index', index: 3 }, 'loop');

    assert.equal(request.vehicles[0].end_index, 3);
  });

  test('gives the vehicle no end in open mode', () => {
    const request = buildVroomRequest(THREE_PLACES_AND_START, 3, { kind: 'index', index: 3 }, 'open');

    assert.equal(request.vehicles[0].end_index, undefined);
  });

  test('starts the vehicle at the departure index', () => {
    const request = buildVroomRequest(THREE_PLACES_AND_START, 3, { kind: 'index', index: 3 }, 'open');

    assert.equal(request.vehicles[0].start_index, 3);
  });

  test('starts from a virtual point at zero distance from every place when the departure is free in open mode', () => {
    const request = buildVroomRequest(THREE_PLACES, 3, { kind: 'none' }, 'open');

    assert.deepEqual(
      { start: request.vehicles[0].start_index, durations: request.matrices.car.durations },
      {
        start: 3,
        durations: [
          [0, 5, 7, 0],
          [5, 0, 4, 0],
          [7, 4, 0, 0],
          [0, 0, 0, 0],
        ],
      },
    );
  });
});

describe('parseVroomSolution', () => {
  test('reads the visit order from the solution', () => {
    const raw = {
      code: 0,
      routes: [
        {
          steps: [
            { type: 'start' },
            { type: 'job', id: 2 },
            { type: 'job', id: 0 },
            { type: 'job', id: 1 },
            { type: 'end' },
          ],
        },
      ],
      unassigned: [],
    };

    assert.deepEqual(parseVroomSolution(raw, 3), [2, 0, 1]);
  });

  test('rejects a solution that leaves a place unassigned', () => {
    const raw = {
      code: 0,
      routes: [{ steps: [{ type: 'start' }, { type: 'job', id: 2 }, { type: 'job', id: 0 }, { type: 'end' }] }],
      unassigned: [{ id: 1, type: 'job' }],
    };

    assert.throws(() => parseVroomSolution(raw, 3), VroomError);
  });
});
