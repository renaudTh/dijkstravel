/** Les vues renvoyées par l'API du studio (`apps/server/src/application/queries.rs`). */

export type ArtifactKind = 'places' | 'tour' | 'video' | 'preview' | 'cover' | 'publication';
export type Profile = 'car' | 'foot' | 'train';
export type Step = 'source' | 'solve' | 'render' | 'publication';
export type JobStatus = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';

export type JobKind =
  | { kind: 'source' }
  | { kind: 'solve' }
  | { kind: 'render'; seconds?: number | null }
  | { kind: 'publication' }
  | { kind: 'all' };

export interface Progress {
  step: Step;
  /** `tiles` ou `frames` pendant un rendu. */
  stage?: 'tiles' | 'frames';
  done: number;
  total: number;
}

export interface Job {
  id: string;
  runId: string;
  runName: string;
  kind: JobKind;
  status: JobStatus;
  progress: Progress | null;
  log: string[];
  error: string | null;
  cancelRequested: boolean;
  createdAt: string;
  startedAt: string | null;
  finishedAt: string | null;
}

export interface Artifact {
  kind: ArtifactKind;
  key: string;
  bytes: number;
  meta: Record<string, unknown>;
}

export interface RunSummary {
  name: string;
  title: string | null;
  profile: Profile;
  mode: 'loop' | 'open';
  format: '16:9' | '9:16';
  theme: string;
  durationSec: number;
  places: number | null;
  stops: number | null;
  meters: number | null;
  artifacts: ArtifactKind[];
  latestJob: Job | null;
}

export interface RunDetail extends RunSummary {
  /** `run.json` tel qu'enregistré. */
  spec: Record<string, unknown>;
  files: Artifact[];
  jobs: Job[];
}

export type RoutingStatus = Record<Profile, boolean>;

/** Corps d'erreur de l'API : `{ "error": "..." }`. */
export interface ApiError {
  error: string;
}

export const FINISHED: readonly JobStatus[] = ['succeeded', 'failed', 'cancelled'];

export function isFinished(job: Pick<Job, 'status'>): boolean {
  return FINISHED.includes(job.status);
}
