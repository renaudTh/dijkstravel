import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { limiter, parseTilePath, tileFetcher } from './tiles.ts';

const JPEG = Buffer.from([0xff, 0xd8, 0xff, 0xe0]);

async function withCache(run: (cacheDir: string) => Promise<void>): Promise<void> {
  const cacheDir = await mkdtemp(join(tmpdir(), 'tiles-'));
  try {
    await run(cacheDir);
  } finally {
    await rm(cacheDir, { recursive: true, force: true });
  }
}

const options = (cacheDir: string) => ({ cacheDir, attempts: 3, retryDelayMs: 1, concurrency: 2 });

describe('parseTilePath', () => {
  test('reads the imagery and the tile coordinates', () => {
    assert.deepEqual(parseTilePath('/imagery/sentinel/12/1351/1864'), { imagery: 'sentinel', z: 12, x: 1351, y: 1864 });
  });

  test('rejects an unknown imagery', () => {
    assert.equal(parseTilePath('/imagery/google/12/1/2'), null);
  });
});

describe('limiter', () => {
  test('never runs more tasks at once than its limit', async () => {
    const queue = limiter(2);
    let running = 0;
    let highest = 0;
    const task = async (): Promise<void> => {
      running++;
      highest = Math.max(highest, running);
      await new Promise((resolve) => setTimeout(resolve, 5));
      running--;
    };

    await Promise.all(Array.from({ length: 6 }, () => queue(task)));

    assert.equal(highest, 2);
  });
});

describe('tileFetcher', () => {
  test('retries a tile whose connection was dropped', async (t) => {
    await withCache(async (cacheDir) => {
      let calls = 0;
      t.mock.method(globalThis, 'fetch', () => {
        calls++;
        return calls === 1 ? Promise.reject(new TypeError('fetch failed')) : Promise.resolve(new Response(JPEG));
      });

      const tile = await tileFetcher(options(cacheDir))('sentinel', 12, 1, 2);

      assert.deepEqual([tile.status, calls], [200, 2]);
    });
  });

  test('serves a tile from the disk cache once fetched', async (t) => {
    await withCache(async (cacheDir) => {
      const fetchMock = t.mock.method(globalThis, 'fetch', () => Promise.resolve(new Response(JPEG)));
      const fetchTile = tileFetcher(options(cacheDir));

      await fetchTile('ign', 15, 3, 4);
      const again = await fetchTile('ign', 15, 3, 4);

      assert.deepEqual([fetchMock.mock.callCount(), again.body], [1, JPEG]);
      assert.deepEqual(await readFile(join(cacheDir, 'ign', '15', '3', '4')), JPEG);
    });
  });

  test('does not retry a tile outside the imagery coverage', async (t) => {
    await withCache(async (cacheDir) => {
      const fetchMock = t.mock.method(globalThis, 'fetch', () => Promise.resolve(new Response('', { status: 404 })));

      const tile = await tileFetcher(options(cacheDir))('nasa', 9, 1, 1);

      assert.deepEqual([tile.status, fetchMock.mock.callCount()], [404, 1]);
    });
  });

  test('gives up after its attempts on a failing server', async (t) => {
    await withCache(async (cacheDir) => {
      const fetchMock = t.mock.method(globalThis, 'fetch', () => Promise.resolve(new Response('', { status: 503 })));

      const tile = await tileFetcher(options(cacheDir))('sentinel', 12, 1, 1);

      assert.deepEqual([tile.status, fetchMock.mock.callCount()], [503, 3]);
    });
  });
});
