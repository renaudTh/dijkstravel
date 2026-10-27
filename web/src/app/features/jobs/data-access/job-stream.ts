import { Injectable } from '@angular/core';
import { Observable } from 'rxjs';

import { Job } from '../../../shared/api/api.models';

/** Attente avant de rouvrir le flux après une coupure (serveur redémarré, mise en veille). */
export const RECONNECT_MS = 3000;

export type JobStreamEvent = { type: 'open' } | { type: 'job'; job: Job };

/** Le flux SSE des jobs (`event: job`). Il se termine en erreur à la première coupure. */
@Injectable({ providedIn: 'root' })
export class JobStream {
  connect(url = '/api/events'): Observable<JobStreamEvent> {
    return new Observable<JobStreamEvent>((subscriber) => {
      const source = new EventSource(url);
      source.onopen = () => subscriber.next({ type: 'open' });
      source.addEventListener('job', (event) =>
        subscriber.next({ type: 'job', job: JSON.parse((event as MessageEvent<string>).data) as Job }),
      );
      // EventSource se reconnecte seul, sans le dire : on ferme et l'effect relance, état resynchronisé.
      source.onerror = () => {
        source.close();
        subscriber.error(new Error('flux des jobs interrompu'));
      };
      return () => source.close();
    });
  }
}
