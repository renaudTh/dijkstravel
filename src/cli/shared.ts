import { readJson } from '../files.ts';
import { parseRun, type Run } from '../run.ts';

/** Imprime les problèmes du run et renvoie `null` s'il est invalide. */
export async function loadRun(file: string): Promise<Run | null> {
  const result = parseRun(await readJson(file));
  if (result.ok) return result.value;
  for (const issue of result.errors) console.error(`${file} : ${issue.path || 'racine'} attendu ${issue.expected}`);
  return null;
}

export function usage(command: string): number {
  console.error(`Usage : ${command}`);
  return 2;
}
