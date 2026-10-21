// Éditeur de thème : réglages à gauche, la vraie page de rendu en aperçu à droite.
import { isRecord } from '../narrow.ts';
import { parseRun, VIDEO_SIZES } from '../run.ts';
import {
  DEFAULT_THEME,
  HUD_FONTS,
  IMAGERIES,
  NUMBER_RANGES,
  parseTheme,
  type Theme,
  type ThemeIssue,
} from '../theme.ts';

const IMAGERY_LABELS: Readonly<Record<string, string>> = {
  nasa: 'NASA Blue Marble (pays, régions)',
  ign: 'Orthophoto IGN (villes, France)',
};

type Field =
  | { readonly kind: 'color'; readonly path: string; readonly label: string }
  | { readonly kind: 'checkbox'; readonly path: string; readonly label: string }
  | { readonly kind: 'range'; readonly path: keyof typeof NUMBER_RANGES; readonly label: string; readonly step: number }
  | { readonly kind: 'font'; readonly path: 'hud.font'; readonly label: string }
  | { readonly kind: 'imagery'; readonly path: 'basemap.imagery'; readonly label: string };

const GROUPS: readonly { readonly title: string; readonly fields: readonly Field[] }[] = [
  {
    title: 'Itinéraire',
    fields: [
      { kind: 'color', path: 'route.color', label: 'Couleur du trait' },
      { kind: 'color', path: 'route.coreColor', label: 'Couleur du cœur' },
      { kind: 'range', path: 'route.width', label: 'Épaisseur (px)', step: 1 },
      { kind: 'range', path: 'route.coreRatio', label: 'Largeur du cœur', step: 0.01 },
      { kind: 'range', path: 'route.coreBlurRatio', label: 'Flou du cœur', step: 0.01 },
    ],
  },
  {
    title: 'Lieux',
    fields: [
      { kind: 'color', path: 'stops.unvisitedColor', label: 'Lieux à venir' },
      { kind: 'range', path: 'stops.unvisitedRadius', label: 'Rayon des lieux à venir', step: 0.5 },
      { kind: 'range', path: 'stops.visitedRatio', label: 'Rayon des lieux atteints', step: 0.05 },
      { kind: 'range', path: 'stops.popRadius', label: 'Ampleur du « pop »', step: 1 },
    ],
  },
  {
    title: 'Carte de fond',
    fields: [
      { kind: 'imagery', path: 'basemap.imagery', label: 'Imagerie' },
      { kind: 'range', path: 'basemap.brightness', label: 'Luminosité', step: 0.01 },
      { kind: 'range', path: 'basemap.contrast', label: 'Contraste', step: 0.01 },
      { kind: 'range', path: 'basemap.saturation', label: 'Saturation', step: 0.01 },
      { kind: 'range', path: 'basemap.hillshade', label: 'Relief', step: 0.01 },
      { kind: 'checkbox', path: 'basemap.borders', label: 'Frontières' },
      { kind: 'color', path: 'basemap.borderColor', label: 'Couleur des frontières' },
      { kind: 'color', path: 'basemap.tintColor', label: 'Teinte de la carte' },
      { kind: 'range', path: 'basemap.tintOpacity', label: 'Intensité de la teinte', step: 0.01 },
    ],
  },
  {
    title: "Carte d'infos",
    fields: [
      { kind: 'font', path: 'hud.font', label: 'Police' },
      { kind: 'range', path: 'hud.size', label: 'Taille du texte (px)', step: 1 },
      { kind: 'color', path: 'hud.background', label: 'Fond' },
      { kind: 'color', path: 'hud.textColor', label: 'Texte' },
      { kind: 'color', path: 'hud.titleColor', label: 'Titre' },
      { kind: 'color', path: 'hud.distanceColor', label: 'Distance' },
      { kind: 'color', path: 'hud.pillFrom', label: 'Pastille : début du dégradé' },
      { kind: 'color', path: 'hud.pillTo', label: 'Pastille : fin du dégradé' },
      { kind: 'color', path: 'hud.pillTextColor', label: 'Pastille : texte' },
      { kind: 'checkbox', path: 'hud.showTitle', label: 'Afficher le titre' },
      { kind: 'checkbox', path: 'hud.showCount', label: 'Afficher le compteur de lieux' },
      { kind: 'checkbox', path: 'hud.showLastStop', label: 'Afficher le dernier lieu' },
    ],
  },
  {
    title: 'Caméra',
    fields: [{ kind: 'range', path: 'camera.followFraction', label: "Part de l'emprise visible", step: 0.005 }],
  },
];

class EditorError extends Error {
  readonly what: string;

  constructor(what: string) {
    super(`Éditeur : ${what}`);
    this.what = what;
  }
}

function element<T extends HTMLElement>(id: string, type: new () => T): T {
  const found = document.getElementById(id);
  if (!(found instanceof type)) throw new EditorError(`#${id}`);
  return found;
}

/** Thème brut modifiable, section par section ; validé par `parseTheme` à chaque application. */
type Draft = Record<string, Record<string, unknown>>;

function toDraft(theme: Theme): Draft {
  return Object.fromEntries(Object.entries(theme).map(([section, values]) => [section, { ...values }]));
}

function valueAt(draft: Draft, path: string): unknown {
  const [section = '', key = ''] = path.split('.');
  return draft[section]?.[key];
}

function setAt(draft: Draft, path: string, value: unknown): void {
  const [section = '', key = ''] = path.split('.');
  draft[section] = { ...draft[section], [key]: value };
}

const formatNumber = (value: number, step: number): string =>
  value.toFixed(step >= 1 ? 0 : (String(step).split('.')[1]?.length ?? 2));

function control(field: Field, draft: Draft, onChange: () => void): HTMLLabelElement {
  const label = document.createElement('label');
  const text = document.createElement('span');
  text.textContent = field.label;
  label.append(text);
  const current = valueAt(draft, field.path);

  if (field.kind === 'font' || field.kind === 'imagery') {
    const select = document.createElement('select');
    const options = field.kind === 'font' ? HUD_FONTS : IMAGERIES;
    for (const option of options)
      select.add(new Option(IMAGERY_LABELS[option] ?? option, option, false, option === current));
    select.addEventListener('change', () => {
      setAt(draft, field.path, select.value);
      onChange();
    });
    label.append(select);
    return label;
  }

  const input = document.createElement('input');
  input.type = field.kind;
  if (field.kind === 'checkbox') {
    input.checked = current === true;
    input.addEventListener('change', () => {
      setAt(draft, field.path, input.checked);
      onChange();
    });
  } else if (field.kind === 'color') {
    input.value = typeof current === 'string' ? current : '#000000';
    input.addEventListener('input', () => {
      setAt(draft, field.path, input.value);
      onChange();
    });
  } else {
    const [min, max] = NUMBER_RANGES[field.path];
    const output = document.createElement('output');
    Object.assign(input, { min: String(min), max: String(max), step: String(field.step) });
    input.value = String(current);
    output.textContent = formatNumber(Number(current), field.step);
    input.addEventListener('input', () => {
      setAt(draft, field.path, Number(input.value));
      output.textContent = formatNumber(Number(input.value), field.step);
      onChange();
    });
    label.append(output);
  }
  label.append(input);
  return label;
}

function renderControls(container: HTMLElement, draft: Draft, onChange: () => void): void {
  container.replaceChildren(
    ...GROUPS.map((group) => {
      const fieldset = document.createElement('fieldset');
      const legend = document.createElement('legend');
      legend.textContent = group.title;
      fieldset.append(legend, ...group.fields.map((field) => control(field, draft, onChange)));
      return fieldset;
    }),
  );
}

async function previewWindow(frame: HTMLIFrameElement): Promise<Window> {
  // Attente active courte : la page d'aperçu n'expose ses fonctions qu'une fois la carte chargée.
  for (;;) {
    const win = frame.contentWindow;
    if (win !== null && win.frameCount > 0) return win;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

function describe(issues: readonly ThemeIssue[]): string {
  return issues.map((issue) => `${issue.path} : ${issue.expected}`).join(' · ');
}

async function start(): Promise<void> {
  const [runResponse, themeResponse] = await Promise.all([fetch('/run.json'), fetch('/theme.json')]);
  const run = parseRun(await runResponse.json());
  const loaded = parseTheme(await themeResponse.json());
  if (!run.ok) throw new EditorError('run.json');
  const status = element('status', HTMLDivElement);
  if (!loaded.ok) status.textContent = `Thème enregistré invalide, valeurs par défaut : ${describe(loaded.errors)}`;
  let draft = toDraft(loaded.ok ? loaded.value : DEFAULT_THEME);

  const size = VIDEO_SIZES[run.value.video.format];
  const frame = element('preview', HTMLIFrameElement);
  const stage = element('stage', HTMLDivElement);
  Object.assign(frame, { width: String(size.width), height: String(size.height) });
  const fit = (): void => {
    const scale = Math.min(stage.clientWidth / size.width, stage.clientHeight / size.height);
    frame.style.transform = `translateX(-50%) scale(${String(scale)})`;
  };
  new ResizeObserver(fit).observe(stage);
  frame.src = '/';
  element('theme-name', HTMLDivElement).textContent = `Run ${run.value.name}`;

  const preview = await previewWindow(frame);
  const slider = element('frame', HTMLInputElement);
  const frameLabel = element('frame-label', HTMLOutputElement);
  slider.max = String(preview.frameCount - 1);
  slider.value = String(Math.round(preview.frameCount / 3));

  // Un seul rendu à la fois ; un réglage arrivé pendant un rendu repart avec les dernières valeurs.
  let rendering = false;
  let pending = false;
  const refresh = async (): Promise<void> => {
    if (rendering) {
      pending = true;
      return;
    }
    rendering = true;
    const issues = await preview.applyTheme(draft);
    status.textContent = issues.length > 0 ? describe(issues) : '';
    frameLabel.textContent = `${slider.value} / ${slider.max}`;
    await preview.renderFrame(Number(slider.value));
    rendering = false;
    if (pending) {
      pending = false;
      await refresh();
    }
  };
  const onChange = (): void => {
    void refresh();
  };
  const controls = element('controls', HTMLDivElement);
  renderControls(controls, draft, onChange);
  slider.addEventListener('input', onChange);

  element('reset', HTMLButtonElement).addEventListener('click', () => {
    draft = toDraft(DEFAULT_THEME);
    renderControls(controls, draft, onChange);
    onChange();
  });
  element('save', HTMLButtonElement).addEventListener('click', () => {
    void (async () => {
      const response = await fetch('/theme.json', { method: 'PUT', body: JSON.stringify(draft) });
      const body: unknown = response.ok ? null : await response.json().catch(() => null);
      status.textContent = response.ok
        ? `Enregistré à ${new Date().toLocaleTimeString('fr-FR')} : la prochaine vidéo l'utilisera.`
        : `Refusé : ${
            Array.isArray(body)
              ? body
                  .filter(isRecord)
                  .map((issue) => String(issue['path']))
                  .join(', ')
              : String(response.status)
          }`;
    })();
  });
  await refresh();
}

void start();
