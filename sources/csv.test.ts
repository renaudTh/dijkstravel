import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { parsePlaces, toPlacesFile } from '../src/places.ts';
import type { Result } from '../src/result.ts';
import { parsePlacesCsv } from './csv.ts';
import type { CsvIssue } from './csv.ts';

const BRITTANY = { west: -5.2, south: 47.2, east: -1.0, north: 48.95 };

function valueOf<T, E>(result: Result<T, E>): T {
  if (!result.ok) assert.fail(`unexpected errors: ${JSON.stringify(result.errors)}`);
  return result.value;
}

function linesOf(result: Result<unknown, CsvIssue>): (number | null)[] {
  return result.ok ? [] : result.errors.map((issue) => ('line' in issue ? issue.line : null));
}

describe('parsePlacesCsv', () => {
  test('converts a conforming CSV into places that pass validation', () => {
    const places = valueOf(parsePlacesCsv('id,name,lat,lon\na,Rennes,48.11,-1.68\nb,Brest,48.39,-4.49\n'));

    assert.equal(parsePlaces(JSON.stringify(toPlacesFile(places)), BRITTANY).ok, true);
  });

  test('stores coordinates in [lon, lat] order', () => {
    const places = valueOf(parsePlacesCsv('id,name,lat,lon\na,Rennes,48.11,-1.68\n'));

    assert.deepEqual(toPlacesFile(places).features[0]?.geometry.coordinates, [-1.68, 48.11]);
  });

  test('reads the optional source column', () => {
    const places = valueOf(parsePlacesCsv('id,name,lat,lon,source\na,Rennes,48.11,-1.68,mairie.fr\n'));

    assert.equal(places[0]?.source, 'mairie.fr');
  });

  test('unquotes a name containing a comma and escaped quotes', () => {
    const places = valueOf(parsePlacesCsv('id,name,lat,lon\nfoix,"Château de Foix, ""le comtal""",42.97,1.61\n'));

    assert.equal(places[0]?.name, 'Château de Foix, "le comtal"');
  });

  test('rejects a quoted decimal comma, citing the line', () => {
    const result = parsePlacesCsv('id,name,lat,lon\na,Rennes,48.11,-1.68\nb,Brest,"48,39",-4.49\n');

    assert.deepEqual(linesOf(result), [3]);
  });

  test('rejects an unquoted decimal comma, citing the line', () => {
    const result = parsePlacesCsv('id,name,lat,lon\na,Rennes,48.11,-1.68\nb,Brest,48,39,-4.49\n');

    assert.deepEqual(linesOf(result), [3]);
  });

  test('rejects a CSV missing a required column, naming it', () => {
    assert.deepEqual(parsePlacesCsv('id,name,lon\na,Rennes,-1.68\n'), {
      ok: false,
      errors: [{ kind: 'missing-column', column: 'lat' }],
    });
  });
});
