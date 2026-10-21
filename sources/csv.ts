import { readText, writeJson } from '../src/files.ts';
import { describePlaceIssue, parsePlaces, toPlacesFile, type Place } from '../src/places.ts';
import type { Result } from '../src/result.ts';
import { runPaths } from '../src/run.ts';
import { loadRun, usage } from '../src/cli/shared.ts';

export const REQUIRED_CSV_COLUMNS = ['id', 'name', 'lat', 'lon'] as const;

export type CsvColumn = (typeof REQUIRED_CSV_COLUMNS)[number];

/** `line` : numéro de la ligne physique où commence l'enregistrement, en-tête = 1. */
export type CsvIssue =
  | { readonly kind: 'missing-column'; readonly column: CsvColumn }
  | { readonly kind: 'field-count'; readonly line: number; readonly expected: number; readonly actual: number }
  | { readonly kind: 'unterminated-quote'; readonly line: number }
  | { readonly kind: 'invalid-number'; readonly line: number; readonly column: 'lat' | 'lon'; readonly value: string };

type CsvRecord = { readonly line: number; readonly fields: readonly string[] };

function parseRecords(text: string): Result<CsvRecord[], CsvIssue> {
  const records: CsvRecord[] = [];
  let fields: string[] = [];
  let field = '';
  let quoted = false;
  let line = 1;
  let recordLine = 1;
  for (let i = 0; i < text.length; i++) {
    const char = text.charAt(i);
    if (quoted) {
      if (char === '"' && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (char === '"') quoted = false;
      else {
        if (char === '\n') line++;
        field += char;
      }
    } else if (char === '"') quoted = true;
    else if (char === ',') {
      fields.push(field);
      field = '';
    } else if (char === '\n' || char === '\r') {
      if (char === '\r' && text[i + 1] === '\n') i++;
      fields.push(field);
      if (fields.some((value) => value !== '')) records.push({ line: recordLine, fields });
      fields = [];
      field = '';
      line++;
      recordLine = line;
    } else field += char;
  }
  if (quoted) return { ok: false, errors: [{ kind: 'unterminated-quote', line: recordLine }] };
  fields.push(field);
  if (fields.some((value) => value !== '')) records.push({ line: recordLine, fields });
  return { ok: true, value: records };
}

/** CSV RFC 4180, séparateur virgule, BOM UTF-8 toléré. */
export function parsePlacesCsv(text: string): Result<Place[], CsvIssue> {
  const parsed = parseRecords(text.replace(/^\uFEFF/, ''));
  if (!parsed.ok) return parsed;
  const [header, ...rows] = parsed.value;
  const columns = (header?.fields ?? []).map((name) => name.trim());
  const missing = REQUIRED_CSV_COLUMNS.filter((column) => !columns.includes(column));
  if (missing.length > 0) return { ok: false, errors: missing.map((column) => ({ kind: 'missing-column', column })) };

  const at = (fields: readonly string[], column: string): string => fields[columns.indexOf(column)] ?? '';
  const issues: CsvIssue[] = [];
  const places: Place[] = [];
  for (const { line, fields } of rows) {
    if (fields.length !== columns.length) {
      issues.push({ kind: 'field-count', line, expected: columns.length, actual: fields.length });
      continue;
    }
    const lat = parseNumber(line, 'lat', at(fields, 'lat'), issues);
    const lon = parseNumber(line, 'lon', at(fields, 'lon'), issues);
    if (lat === null || lon === null) continue;
    const source = columns.includes('source') ? at(fields, 'source') : '';
    places.push({
      id: at(fields, 'id'),
      name: at(fields, 'name'),
      at: [lon, lat],
      source: source === '' ? null : source,
    });
  }
  return issues.length > 0 ? { ok: false, errors: issues } : { ok: true, value: places };
}

function parseNumber(line: number, column: 'lat' | 'lon', value: string, issues: CsvIssue[]): number | null {
  const number = /^\s*-?\d+(\.\d+)?\s*$/.test(value) ? Number(value) : NaN;
  if (Number.isFinite(number)) return number;
  issues.push({ kind: 'invalid-number', line, column, value });
  return null;
}

export function describeCsvIssue(issue: CsvIssue): string {
  switch (issue.kind) {
    case 'missing-column':
      return `Colonne manquante dans l'en-tête : ${issue.column}.`;
    case 'field-count':
      return `Ligne ${String(issue.line)} : ${String(issue.actual)} champs au lieu de ${String(issue.expected)} (virgule décimale non protégée ?).`;
    case 'unterminated-quote':
      return `Ligne ${String(issue.line)} : guillemet ouvert jamais refermé.`;
    case 'invalid-number':
      return `Ligne ${String(issue.line)} : ${issue.column} "${issue.value}" n'est pas un nombre décimal à point.`;
  }
}

/** `node sources/csv.ts <run.json> <lieux.csv>` → `data/<run>/places.geojson`. */
export async function main(argv: readonly string[]): Promise<number> {
  const [runFile, csvFile] = argv;
  if (runFile === undefined || csvFile === undefined) return usage('source:csv <run.json> <lieux.csv>');
  const run = await loadRun(runFile);
  if (run === null) return 1;
  const parsed = parsePlacesCsv(await readText(csvFile));
  if (!parsed.ok) {
    for (const issue of parsed.errors) console.error(describeCsvIssue(issue));
    return 1;
  }
  const file = toPlacesFile(parsed.value);
  const validated = parsePlaces(JSON.stringify(file), run.bbox);
  if (!validated.ok) {
    for (const issue of validated.errors) console.error(describePlaceIssue(issue));
    return 1;
  }
  const out = runPaths(run).places;
  await writeJson(out, file);
  console.log(`${out} : ${String(validated.value.length)} lieux.`);
  return 0;
}

if (import.meta.main) process.exitCode = await main(process.argv.slice(2));
