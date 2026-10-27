import { JobKind, JobStatus, Progress, Step } from '../../../shared/api/api.models';

const kilometres = new Intl.NumberFormat('fr-FR', { maximumFractionDigits: 0 });

/** `25 469 km`, comme la légende de publication. */
export function formatKm(meters: number | null): string {
  return meters === null ? '–' : `${kilometres.format(meters / 1000)} km`;
}

export const STATUS_LABELS: Record<JobStatus, string> = {
  queued: 'en file',
  running: 'en cours',
  succeeded: 'terminé',
  failed: 'échec',
  cancelled: 'annulé',
};

export const STEP_LABELS: Record<Step, string> = {
  source: 'Sourçage',
  solve: 'Calcul de la tournée',
  render: 'Rendu',
  publication: 'Publication',
};

export function kindLabel(kind: JobKind): string {
  switch (kind.kind) {
    case 'source':
      return 'Sourçage';
    case 'solve':
      return 'Calcul';
    case 'render':
      return kind.seconds ? `Aperçu ${kind.seconds} s` : 'Rendu';
    case 'publication':
      return 'Publication';
    case 'all':
      return 'Tout faire';
  }
}

/** `Rendu · images 804 / 3750`. */
export function progressLabel(progress: Progress): string {
  const stage = progress.stage === 'tiles' ? 'tuiles' : progress.stage === 'frames' ? 'images' : null;
  const detail = stage ? ` · ${stage} ${progress.done} / ${progress.total}` : '';
  return `${STEP_LABELS[progress.step]}${detail}`;
}

/** Part faite, de 0 à 100 ; `null` quand l'étape n'a pas d'avancement mesurable. */
export function progressPercent(progress: Progress | null): number | null {
  if (!progress || !progress.stage || progress.total === 0) return null;
  return Math.round((progress.done / progress.total) * 100);
}
