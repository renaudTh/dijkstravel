import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { placesFromElements } from './overpass.ts';
import type { OverpassElement } from './overpass.ts';

// 0.0003° de latitude ≈ 33 m, 0.00055° ≈ 61 m.
const NEAR = 0.0003;
const FAR = 0.00055;

function node(id: number, lat: number, lon: number, tags: Record<string, string> = {}): OverpassElement {
  return { type: 'node', id, lat, lon, tags };
}

describe('placesFromElements', () => {
  test('keeps a node at its coordinates', () => {
    const { places } = placesFromElements([node(1, 48.11, -1.68, { name: 'Rennes' })]);

    assert.deepEqual(places[0]?.at, [-1.68, 48.11]);
  });

  test('places a way at its center', () => {
    const { places } = placesFromElements([
      { type: 'way', id: 2, center: { lat: 48.39, lon: -4.49 }, tags: { name: 'Château de Brest' } },
    ]);

    assert.deepEqual(places[0]?.at, [-4.49, 48.39]);
  });

  test('places a relation at its center', () => {
    const { places } = placesFromElements([
      { type: 'relation', id: 3, center: { lat: 47.66, lon: -2.76 }, tags: { name: 'Remparts de Vannes' } },
    ]);

    assert.deepEqual(places[0]?.at, [-2.76, 47.66]);
  });

  test('builds ids as osm:<type>/<id>', () => {
    const { places } = placesFromElements([
      node(1, 48.11, -1.68, { name: 'Rennes' }),
      { type: 'way', id: 2, center: { lat: 48.39, lon: -4.49 }, tags: { name: 'Brest' } },
      { type: 'relation', id: 3, center: { lat: 47.66, lon: -2.76 }, tags: { name: 'Vannes' } },
    ]);

    assert.deepEqual(places.map((place) => place.id).toSorted(), ['osm:node/1', 'osm:relation/3', 'osm:way/2']);
  });

  test('merges two places closer than 50 m', () => {
    const { places } = placesFromElements([
      node(1, 48.11, -1.68, { name: 'Château' }),
      node(2, 48.11 + NEAR, -1.68, { name: 'Château (entrée)' }),
    ]);

    assert.equal(places.length, 1);
  });

  test('keeps the named place when merging', () => {
    const { places } = placesFromElements([
      node(1, 48.11, -1.68, { historic: 'castle' }),
      node(2, 48.11 + NEAR, -1.68, { historic: 'castle', name: 'Château' }),
    ]);

    assert.deepEqual(
      places.map((place) => place.id),
      ['osm:node/2'],
    );
  });

  test('keeps two places 60 m apart', () => {
    const { places } = placesFromElements([
      node(1, 48.11, -1.68, { name: 'Château' }),
      node(2, 48.11 + FAR, -1.68, { name: 'Manoir' }),
    ]);

    assert.equal(places.length, 2);
  });

  test('drops a place without name tag', () => {
    const { places } = placesFromElements([
      node(1, 48.11, -1.68, { name: 'Rennes' }),
      node(2, 48.39, -4.49, { historic: 'castle' }),
    ]);

    assert.deepEqual(
      places.map((place) => place.id),
      ['osm:node/1'],
    );
  });

  test('lists the ids of the places dropped for lack of a name', () => {
    const { unnamed } = placesFromElements([
      node(1, 48.11, -1.68, { name: 'Rennes' }),
      node(2, 48.39, -4.49, { historic: 'castle' }),
      { type: 'way', id: 3, center: { lat: 47.66, lon: -2.76 }, tags: { historic: 'castle' } },
    ]);

    assert.deepEqual(unnamed.toSorted(), ['osm:node/2', 'osm:way/3']);
  });
});
