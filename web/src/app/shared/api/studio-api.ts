import { HttpClient, HttpErrorResponse } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

import { ApiError, ArtifactKind, Job, JobKind, RoutingStatus, RunDetail, RunSummary } from './api.models';

const FILE_NAMES: Record<ArtifactKind, string> = {
  places: 'places.geojson',
  tour: 'tour.json',
  video: 'video.mp4',
  preview: 'preview.mp4',
  cover: 'cover.jpg',
  publication: 'publication.txt',
};

/** Client HTTP du studio : utilisé par les effects seuls, jamais par les composants. */
@Injectable({ providedIn: 'root' })
export class StudioApi {
  private readonly http = inject(HttpClient);

  runs(): Observable<RunSummary[]> {
    return this.http.get<RunSummary[]>('/api/runs');
  }

  run(name: string): Observable<RunDetail> {
    return this.http.get<RunDetail>(`/api/runs/${encodeURIComponent(name)}`);
  }

  enqueue(name: string, kind: JobKind): Observable<Job> {
    return this.http.post<Job>(`/api/runs/${encodeURIComponent(name)}/jobs`, kind);
  }

  cancel(id: string): Observable<Job> {
    return this.http.post<Job>(`/api/jobs/${id}/cancel`, null);
  }

  routing(): Observable<RoutingStatus> {
    return this.http.get<RoutingStatus>('/api/routing');
  }

  text(name: string, kind: ArtifactKind): Observable<string> {
    return this.http.get(fileUrl(name, kind), { responseType: 'text' });
  }
}

export function fileUrl(name: string, kind: ArtifactKind, version?: string | number): string {
  const url = `/api/runs/${encodeURIComponent(name)}/files/${FILE_NAMES[kind]}`;
  return version === undefined ? url : `${url}?v=${encodeURIComponent(String(version))}`;
}

/** Le message à montrer pour une erreur HTTP : celui de l'API s'il y en a un. */
export function errorMessage(error: unknown): string {
  if (error instanceof HttpErrorResponse) {
    const body = error.error as Partial<ApiError> | string | null;
    if (body && typeof body === 'object' && body.error) return body.error;
    if (error.status === 0) return 'Studio injoignable';
    return `HTTP ${error.status}`;
  }
  return String(error);
}
