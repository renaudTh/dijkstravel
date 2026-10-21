import { readJson } from '../files.ts';
import { parseRun, runThemeName, type Run } from '../run.ts';
import { themePath } from '../theme.ts';

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

/** Fichier du thème que référence le run ; `null` (après message) si son nom est invalide. */
export async function loadThemeFile(file: string): Promise<string | null> {
  const name = runThemeName(await readJson(file));
  if (name === null) console.error(`${file} : theme attendu nom de fichier (lettres, chiffres, - et _)`);
  return name === null ? null : themePath(name);
}
