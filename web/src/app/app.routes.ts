import { Routes } from '@angular/router';

import { RunPage } from './features/runs/ui/run-page';
import { RunsPage } from './features/runs/ui/runs-page';

export const routes: Routes = [
  { path: '', pathMatch: 'full', redirectTo: 'runs' },
  { path: 'runs', component: RunsPage, title: 'Runs · Dijkstravel' },
  { path: 'runs/:name', component: RunPage, title: 'Run · Dijkstravel' },
  { path: '**', redirectTo: 'runs' },
];
