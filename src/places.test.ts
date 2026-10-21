import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import type { BBox } from './geo.ts';
import { describePlaceIssue, parsePlaces } from './places.ts';
import type { PlaceIssue } from './places.ts';

const BRITTANY: BBox = { west: -5.2, south: 47.2, east: -1.0, north: 48.95 };

function point(id: unknown, name: unknown, coordinates: unknown, extra: Record<string, unknown> = {}): unknown {
  return {
    type: 'Feature',
    geometry: { type: 'Point', coordinates },
    properties: { id, name, ...extra },
  };
}

function collection(...features: unknown[]): string {
  return JSON.stringify({ type: 'FeatureCollection', features });
}

describe('parsePlaces', () => {
  test('accepts a conforming FeatureCollection and keeps the file order', () => {
    const text = collection(
      point('b', 'Brest', [-4.49, 48.39], { source: 'manual' }),
      point('a', 'Rennes', [-1.68, 48.11], { source: 'manual' }),
      point('c', 'Vannes', [-2.76, 47.66], { source: 'osm' }),
    );

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: true,
      value: [
        { id: 'b', name: 'Brest', at: [-4.49, 48.39], source: 'manual' },
        { id: 'a', name: 'Rennes', at: [-1.68, 48.11], source: 'manual' },
        { id: 'c', name: 'Vannes', at: [-2.76, 47.66], source: 'osm' },
      ],
    });
  });

  test('accepts a feature without source', () => {
    const text = collection(point('a', 'Rennes', [-1.68, 48.11]));

    assert.equal(parsePlaces(text, BRITTANY).ok, true);
  });

  test('ignores unknown properties', () => {
    const text = collection(point('a', 'Rennes', [-1.68, 48.11], { city: 'Rennes' }));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: true,
      value: [{ id: 'a', name: 'Rennes', at: [-1.68, 48.11], source: null }],
    });
  });

  test('rejects text that is not JSON', () => {
    assert.deepEqual(parsePlaces('{"type": "FeatureCollection",', BRITTANY), {
      ok: false,
      errors: [{ kind: 'not-json' }],
    });
  });

  test('rejects a root that is not a FeatureCollection', () => {
    const text = JSON.stringify(point('a', 'Rennes', [-1.68, 48.11]));

    assert.deepEqual(parsePlaces(text, BRITTANY), { ok: false, errors: [{ kind: 'not-feature-collection' }] });
  });

  test('rejects a FeatureCollection without a features array', () => {
    const text = JSON.stringify({ type: 'FeatureCollection' });

    assert.deepEqual(parsePlaces(text, BRITTANY), { ok: false, errors: [{ kind: 'not-feature-collection' }] });
  });

  test('rejects a polygon geometry, citing the feature index and id', () => {
    const polygon = {
      type: 'Feature',
      geometry: {
        type: 'Polygon',
        coordinates: [
          [
            [-1.7, 48.1],
            [-1.6, 48.1],
            [-1.6, 48.2],
            [-1.7, 48.1],
          ],
        ],
      },
      properties: { id: 'b', name: 'Parc' },
    };
    const text = collection(point('a', 'Rennes', [-1.68, 48.11]), polygon);

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'not-a-point', index: 1, id: 'b' }],
    });
  });

  test('rejects a feature without geometry, citing its index', () => {
    const text = collection(point('a', 'Rennes', [-1.68, 48.11]), {
      type: 'Feature',
      properties: { id: 'b', name: 'Brest' },
    });

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'not-a-point', index: 1, id: 'b' }],
    });
  });

  test('rejects a feature without id, citing its index', () => {
    const text = collection(point('a', 'Rennes', [-1.68, 48.11]), {
      type: 'Feature',
      geometry: { type: 'Point', coordinates: [-4.49, 48.39] },
      properties: { name: 'Brest' },
    });

    assert.deepEqual(parsePlaces(text, BRITTANY), { ok: false, errors: [{ kind: 'missing-id', index: 1 }] });
  });

  test('rejects an empty id', () => {
    const text = collection(point('', 'Rennes', [-1.68, 48.11]));

    assert.deepEqual(parsePlaces(text, BRITTANY), { ok: false, errors: [{ kind: 'missing-id', index: 0 }] });
  });

  test('rejects a non-string id', () => {
    const text = collection(point(42, 'Rennes', [-1.68, 48.11]));

    assert.deepEqual(parsePlaces(text, BRITTANY), { ok: false, errors: [{ kind: 'missing-id', index: 0 }] });
  });

  test('rejects a duplicated id, citing both indexes', () => {
    const text = collection(
      point('a', 'Rennes', [-1.68, 48.11]),
      point('b', 'Brest', [-4.49, 48.39]),
      point('a', 'Vannes', [-2.76, 47.66]),
    );

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'duplicate-id', id: 'a', indexes: [0, 2] }],
    });
  });

  test('rejects a missing name, citing the feature index and id', () => {
    const text = collection(point('a', 'Rennes', [-1.68, 48.11]), {
      type: 'Feature',
      geometry: { type: 'Point', coordinates: [-4.49, 48.39] },
      properties: { id: 'b' },
    });

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'missing-name', index: 1, id: 'b' }],
    });
  });

  test('rejects a whitespace-only name', () => {
    const text = collection(point('a', '   ', [-1.68, 48.11]));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'missing-name', index: 0, id: 'a' }],
    });
  });

  test('rejects a longitude outside -180 to 180', () => {
    const text = collection(point('a', 'Rennes', [181, 48.11]));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'invalid-coordinates', index: 0, id: 'a' }],
    });
  });

  test('rejects a latitude outside -90 to 90', () => {
    const text = collection(point('a', 'Rennes', [-1.68, 91]));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'invalid-coordinates', index: 0, id: 'a' }],
    });
  });

  test('rejects a non-numeric coordinate', () => {
    const text = collection(point('a', 'Rennes', ['-1.68', 48.11]));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'invalid-coordinates', index: 0, id: 'a' }],
    });
  });

  test('rejects a place outside the run bbox, citing the feature index and id', () => {
    const text = collection(point('a', 'Rennes', [-1.68, 48.11]), point('paris', 'Paris', [2.35, 48.85]));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'outside-bbox', index: 1, id: 'paris', swapped: false }],
    });
  });

  test('flags a probable lat/lon swap when the swapped point falls inside the bbox', () => {
    const text = collection(point('a', 'Rennes', [48.11, -1.68]));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'outside-bbox', index: 0, id: 'a', swapped: true }],
    });
  });

  test('does not flag a swap when the swapped point is outside the bbox as well', () => {
    const text = collection(point('berlin', 'Berlin', [13.4, 52.52]));

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [{ kind: 'outside-bbox', index: 0, id: 'berlin', swapped: false }],
    });
  });

  test('mentions the probable lat/lon swap in the issue description', () => {
    const description = describePlaceIssue({ kind: 'outside-bbox', index: 0, id: 'a', swapped: true });

    assert.match(description, /invers|permut|swap/i);
  });

  test('cites the feature index and id in the issue description', () => {
    const issues: PlaceIssue[] = [
      { kind: 'not-a-point', index: 7, id: 'castle-foix' },
      { kind: 'missing-name', index: 7, id: 'castle-foix' },
      { kind: 'invalid-coordinates', index: 7, id: 'castle-foix' },
      { kind: 'outside-bbox', index: 7, id: 'castle-foix', swapped: false },
    ];

    const uncited = issues.filter((issue) => {
      const description = describePlaceIssue(issue);
      return !(description.includes('castle-foix') && /\b7\b/.test(description));
    });

    assert.deepEqual(uncited, []);
  });

  test('reports every faulty feature, not only the first', () => {
    const text = collection(
      point('a', '', [-1.68, 48.11]),
      point('b', 'Brest', [-4.49, 48.39]),
      point('c', 'Vannes', [-2.76, 95]),
    );

    assert.deepEqual(parsePlaces(text, BRITTANY), {
      ok: false,
      errors: [
        { kind: 'missing-name', index: 0, id: 'a' },
        { kind: 'invalid-coordinates', index: 2, id: 'c' },
      ],
    });
  });

  test('rejects a FeatureCollection without features', () => {
    assert.deepEqual(parsePlaces(collection(), BRITTANY), { ok: false, errors: [{ kind: 'empty' }] });
  });
});
