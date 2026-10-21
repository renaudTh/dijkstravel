import { isFiniteNumber, isRecord } from '../narrow.ts';
import type { TourMode } from '../run.ts';
import type { DistanceMatrix } from './osrm.ts';

/**
 * Départ résolu en indice de matrice. `none` : départ libre en trajet ouvert,
 * modélisé par un point fictif à distance nulle de tous les lieux.
 */
export type Departure = { readonly kind: 'index'; readonly index: number } | { readonly kind: 'none' };

/** Requête VROOM sur matrice fournie : VROOM n'appelle pas OSRM. */
export type VroomRequest = {
  readonly vehicles: readonly [
    { readonly id: number; readonly profile: 'car'; readonly start_index?: number; readonly end_index?: number },
  ];
  readonly jobs: readonly { readonly id: number; readonly location_index: number }[];
  readonly matrices: { readonly car: { readonly durations: readonly (readonly number[])[] } };
};

export class VroomError extends Error {
  readonly reason: string;

  constructor(reason: string) {
    super(`VROOM : ${reason}`);
    this.reason = reason;
  }
}

/** `matrix` couvre les `jobCount` lieux, puis éventuellement le point de départ libre. */
export function buildVroomRequest(
  matrix: DistanceMatrix,
  jobCount: number,
  departure: Departure,
  mode: TourMode,
): VroomRequest {
  const size = matrix.length;
  const filled = matrix.map((row, from) =>
    row.map((cell, to) => {
      if (cell === null) throw new VroomError(`aucune route de ${String(from)} vers ${String(to)}`);
      return Math.round(cell);
    }),
  );
  // VROOM minimise `durations` par défaut : on lui donne des mètres pour obtenir le plus court (D-4).
  const durations =
    departure.kind === 'none' ? [...filled.map((row) => [...row, 0]), new Array<number>(size + 1).fill(0)] : filled;
  const startIndex = departure.kind === 'none' ? size : departure.index;
  const vehicle =
    mode === 'loop'
      ? { id: 0, profile: 'car' as const, start_index: startIndex, end_index: startIndex }
      : { id: 0, profile: 'car' as const, start_index: startIndex };
  return {
    vehicles: [vehicle],
    jobs: Array.from({ length: jobCount }, (_, index) => ({ id: index, location_index: index })),
    matrices: { car: { durations } },
  };
}

/** Indices des lieux dans l'ordre de visite. */
export function parseVroomSolution(raw: unknown, jobCount: number): number[] {
  if (!isRecord(raw)) throw new VroomError('réponse illisible');
  if (raw['code'] !== 0) throw new VroomError(`code ${String(raw['code'])} : ${String(raw['error'])}`);
  const unassigned = raw['unassigned'];
  if (Array.isArray(unassigned) && unassigned.length > 0) {
    throw new VroomError(`${String(unassigned.length)} lieux non assignés`);
  }
  const routes = raw['routes'];
  const route: unknown = Array.isArray(routes) ? routes[0] : undefined;
  const steps = isRecord(route) ? route['steps'] : undefined;
  if (!Array.isArray(steps)) throw new VroomError('aucune route dans la solution');
  const rawSteps: readonly unknown[] = steps;
  const order = rawSteps.flatMap((step) =>
    isRecord(step) && step['type'] === 'job' && isFiniteNumber(step['id']) ? [step['id']] : [],
  );
  if (order.length !== jobCount || new Set(order).size !== jobCount) {
    throw new VroomError(`${String(order.length)} lieux visités sur ${String(jobCount)}`);
  }
  return order;
}

export async function postVroom(vroomUrl: string, request: VroomRequest): Promise<unknown> {
  const response = await fetch(`${vroomUrl}/`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(request),
  });
  const raw: unknown = await response.json().catch(() => {
    throw new VroomError(`HTTP ${String(response.status)} sans corps JSON`);
  });
  return raw;
}
