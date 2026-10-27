import { provideHttpClient, withFetch } from '@angular/common/http';
import { ApplicationConfig, inject, isDevMode, provideAppInitializer, provideBrowserGlobalErrorListeners } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { provideRouter, withComponentInputBinding } from '@angular/router';
import { provideEffects } from '@ngrx/effects';
import { provideRouterStore, routerReducer } from '@ngrx/router-store';
import { provideStore } from '@ngrx/store';
import { provideStoreDevtools } from '@ngrx/store-devtools';

import { routes } from './app.routes';
import * as jobsEffects from './features/jobs/data-access/jobs.effects';
import { jobsFeature } from './features/jobs/data-access/jobs.reducer';
import * as routingEffects from './features/routing/data-access/routing.state';
import { routingFeature } from './features/routing/data-access/routing.state';
import * as runsEffects from './features/runs/data-access/runs.effects';
import { runsFeature } from './features/runs/data-access/runs.reducer';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes, withComponentInputBinding()),
    provideHttpClient(withFetch()),
    provideStore({
      router: routerReducer,
      [runsFeature.name]: runsFeature.reducer,
      [jobsFeature.name]: jobsFeature.reducer,
      [routingFeature.name]: routingFeature.reducer,
    }),
    provideRouterStore(),
    provideEffects(runsEffects, jobsEffects, { pollRouting$: routingEffects.pollRouting$ }),
    provideStoreDevtools({ maxAge: 50, logOnly: !isDevMode() }),
    // Les icônes viennent de la police Material Symbols (index.html).
    provideAppInitializer(() => {
      inject(MatIconRegistry).setDefaultFontSetClass('material-symbols-outlined');
    }),
  ],
};
