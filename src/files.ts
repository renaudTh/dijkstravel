import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

export function readText(file: string): Promise<string> {
  return readFile(file, 'utf8');
}

export async function readJson(file: string): Promise<unknown> {
  const raw: unknown = JSON.parse(await readText(file));
  return raw;
}

/** Crée le dossier parent au besoin. */
export async function writeJson(file: string, value: unknown): Promise<void> {
  await mkdir(dirname(file), { recursive: true });
  await writeFile(file, JSON.stringify(value));
}
