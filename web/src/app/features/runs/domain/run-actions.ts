import { Job, JobKind, RoutingStatus, RunDetail, isFinished } from '../../../shared/api/api.models';

/**
 * Les actions proposées sur un run, et pourquoi l'une est indisponible. Mêmes règles que le serveur
 * (`RunProduction::can_enqueue`), pour griser un bouton avant d'essuyer un refus.
 */
export interface RunAction {
  id: 'source' | 'solve' | 'render' | 'preview' | 'publication' | 'all';
  label: string;
  icon: string;
  kind: JobKind;
  enabled: boolean;
  /** Pourquoi l'action est indisponible. */
  reason?: string;
}

export const PREVIEW_SECONDS = 10;

const PROFILE_LABELS = { car: 'voiture', foot: 'piéton', train: 'train' } as const;

export function runActions(
  run: RunDetail,
  activeJob: Job | null,
  routing: RoutingStatus | null,
): RunAction[] {
  const has = (kind: RunDetail['artifacts'][number]) => run.artifacts.includes(kind);
  const busy = activeJob && !isFinished(activeJob) ? 'un job est déjà en cours sur ce run' : undefined;
  const routingDown =
    routing && !routing[run.profile]
      ? `routage ${PROFILE_LABELS[run.profile]} arrêté (OSRM ou VROOM)`
      : undefined;
  const noPlaces = has('places') ? undefined : 'lieux absents : sourcer le run d’abord';
  const noTour = has('tour') ? undefined : 'tournée absente : la calculer d’abord';

  const action = (
    id: RunAction['id'],
    label: string,
    icon: string,
    kind: JobKind,
    ...blockers: (string | undefined)[]
  ): RunAction => {
    const reason = [busy, ...blockers].find((blocker) => blocker !== undefined);
    return { id, label, icon, kind, enabled: reason === undefined, reason };
  };

  return [
    action(
      'source',
      'Sourcer',
      'travel_explore',
      { kind: 'source' },
      run.spec['overpass'] ? undefined : 'pas de section overpass : lieux d’une autre source',
    ),
    action('solve', 'Calculer', 'route', { kind: 'solve' }, noPlaces, routingDown),
    action('preview', `Aperçu ${PREVIEW_SECONDS} s`, 'slow_motion_video', { kind: 'render', seconds: PREVIEW_SECONDS }, noTour),
    action('render', 'Rendre', 'movie', { kind: 'render' }, noTour),
    action(
      'publication',
      'Publication',
      'ios_share',
      { kind: 'publication' },
      noTour,
      run.spec['publication'] ? undefined : 'pas de section publication (légende, hashtags)',
    ),
    action('all', 'Tout faire', 'bolt', { kind: 'all' }, noPlaces, routingDown),
  ];
}
