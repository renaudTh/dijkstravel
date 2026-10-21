import { afterEach, beforeEach, describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { main } from './validate.ts';

const RUN_NAME = 'validate-test';

function point(id: string, name: string, coordinates: readonly [number, number]): unknown {
  return { type: 'Feature', geometry: { type: 'Point', coordinates }, properties: { id, name } };
}

describe('validate CLI', () => {
  const initialDir = process.cwd();
  let dir = '';
  let runFile = '';

  beforeEach(async () => {
    dir = await mkdtemp(path.join(tmpdir(), 'dijkstravel-validate-'));
    runFile = path.join(dir, 'run.json');
    await writeFile(
      runFile,
      JSON.stringify({
        name: RUN_NAME,
        bbox: { west: -5.2, south: 47.2, east: -1.0, north: 48.95 },
        mode: 'loop',
      }),
    );
    await mkdir(path.join(dir, 'data', RUN_NAME), { recursive: true });
    process.chdir(dir);
  });

  afterEach(async () => {
    process.chdir(initialDir);
    await rm(dir, { recursive: true, force: true });
  });

  async function writePlaces(...features: unknown[]): Promise<void> {
    await writeFile(
      path.join(dir, 'data', RUN_NAME, 'places.geojson'),
      JSON.stringify({ type: 'FeatureCollection', features }),
    );
  }

  test('exits with 0 on a conforming places file', async () => {
    await writePlaces(point('a', 'Rennes', [-1.68, 48.11]), point('b', 'Brest', [-4.49, 48.39]));

    assert.equal(await main([runFile]), 0);
  });

  test('exits with a non-zero code on a faulty places file', async (t) => {
    t.mock.method(console, 'error', () => {});
    await writePlaces(point('a', '', [-1.68, 48.11]));

    assert.notEqual(await main([runFile]), 0);
  });

  test('prints every issue of a faulty places file', async (t) => {
    const error = t.mock.method(console, 'error', () => {});
    await writePlaces(
      point('a', '', [-1.68, 48.11]),
      point('b', 'Brest', [-4.49, 48.39]),
      point('c', 'Paris', [2.35, 48.85]),
    );

    await main([runFile]);

    assert.equal(error.mock.callCount(), 2);
  });
});
