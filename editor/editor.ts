// Éditeur de thème : réglages à gauche, à droite l'image du moteur de rendu, telle que la vidéo la montrera.
// Toute la logique est côté serveur (`dijkstravel editor`) : il donne le thème complété, les bornes des
// réglages et le nombre d'images, valide chaque aperçu et chaque enregistrement. Ici, seulement l'interface.

type Issue = { readonly path: string; readonly expected: string };

/** Thème brut modifiable, section par section ; validé par le serveur à chaque aperçu. */
type Draft = Record<string, Record<string, unknown>>;

/** `GET /editor.json`. */
type EditorState = {
  readonly run: { readonly name: string; readonly width: number; readonly height: number; readonly frames: number };
  readonly theme: Draft;
  /** Problèmes du thème enregistré, remplacé par les valeurs par défaut. */
  readonly issues: readonly Issue[];
  readonly defaults: Draft;
  readonly ranges: Readonly<Record<string, readonly [number, number]>>;
  readonly fonts: readonly string[];
  readonly imageries: readonly string[];
};

function isRecord(value: unknown): value is Readonly<Record<string, unknown>> {
  return typeof value === 'object' && value !== null && !Array.isArray(value);
}

const isDraft = (raw: unknown): raw is Draft => isRecord(raw) && Object.values(raw).every(isRecord);

const isStrings = (raw: unknown): raw is readonly string[] =>
  Array.isArray(raw) && raw.every((item) => typeof item === 'string');

const isRange = (raw: unknown): raw is readonly [number, number] =>
  Array.isArray(raw) && raw.length === 2 && raw.every((item) => typeof item === 'number');

function isEditorState(raw: unknown): raw is EditorState {
  if (!isRecord(raw)) return false;
  const { run, theme, issues, defaults, ranges, fonts, imageries } = raw;
  return (
    isRecord(run) &&
    typeof run['name'] === 'string' &&
    typeof run['width'] === 'number' &&
    typeof run['height'] === 'number' &&
    typeof run['frames'] === 'number' &&
    isDraft(theme) &&
    isDraft(defaults) &&
    Array.isArray(issues) &&
    issues.every(isIssue) &&
    isRecord(ranges) &&
    Object.values(ranges).every(isRange) &&
    isStrings(fonts) &&
    isStrings(imageries)
  );
}

const IMAGERY_LABELS: Readonly<Record<string, string>> = {
  nasa: 'NASA Blue Marble (pays, régions)',
  ign: 'Orthophoto IGN (villes, France)',
};

type Field =
  | { readonly kind: 'color'; readonly path: string; readonly label: string }
  | { readonly kind: 'checkbox'; readonly path: string; readonly label: string }
  | { readonly kind: 'text'; readonly path: string; readonly label: string }
  | { readonly kind: 'range'; readonly path: string; readonly label: string; readonly step: number }
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
      { kind: 'checkbox', path: 'route.stripes', label: 'Trois bandes (trait, cœur, bande droite)' },
      { kind: 'color', path: 'route.stripeColor', label: 'Couleur de la bande droite' },
      { kind: 'color', path: 'route.ferryColor', label: 'Couleur des traversées (bac, pirogue)' },
      { kind: 'color', path: 'route.timeTrialColor', label: 'Couleur des contre-la-montre (cœur en tirets)' },
      { kind: 'range', path: 'route.headRadius', label: 'Rayon de la tête (0 : aucune)', step: 1 },
      { kind: 'color', path: 'route.headColor', label: 'Couleur de la tête' },
      { kind: 'text', path: 'route.headIcon', label: 'Emoji à la tête (⚽…), à la place du point' },
    ],
  },
  {
    title: 'Lieux',
    fields: [
      { kind: 'color', path: 'stops.unvisitedColor', label: 'Lieux à venir' },
      { kind: 'range', path: 'stops.unvisitedRadius', label: 'Rayon des lieux à venir', step: 0.5 },
      { kind: 'text', path: 'stops.startColor', label: 'Couleur du départ (#rrggbb, vide : aucune)' },
      { kind: 'text', path: 'stops.endColor', label: "Couleur de l'arrivée (#rrggbb, vide : aucune)" },
      { kind: 'color', path: 'stops.sprintColor', label: 'Sprints intermédiaires' },
      { kind: 'color', path: 'stops.climbColor', label: 'Ascensions classées' },
      { kind: 'range', path: 'stops.visitedRatio', label: 'Rayon des lieux atteints', step: 0.05 },
      { kind: 'range', path: 'stops.popRadius', label: 'Ampleur du « pop »', step: 1 },
      { kind: 'text', path: 'stops.icon', label: 'Emoji sur les lieux atteints (🏟️, 🍺…)' },
      { kind: 'range', path: 'stops.iconSize', label: "Taille de l'emoji (px)", step: 1 },
      { kind: 'checkbox', path: 'stops.showImageOnMap', label: 'Image du lieu (blason…) sur la carte' },
      { kind: 'range', path: 'stops.imageSize', label: "Taille de l'image sur la carte (px)", step: 1 },
      { kind: 'checkbox', path: 'stops.showLabels', label: 'Nom des lieux atteints sur la carte' },
      { kind: 'checkbox', path: 'stops.showLocalities', label: 'Nom des localités traversées' },
      { kind: 'range', path: 'stops.labelSize', label: 'Taille des noms (px)', step: 1 },
      { kind: 'range', path: 'stops.markerLabelSize', label: 'Taille des sprints et ascensions (px)', step: 1 },
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
      { kind: 'color', path: 'basemap.backgroundColor', label: 'Fond hors imagerie (mer IGN)' },
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
      { kind: 'checkbox', path: 'hud.showImage', label: "Afficher l'image du lieu (blason…)" },
      { kind: 'checkbox', path: 'hud.showProfile', label: "Afficher le profil d'altitude" },
    ],
  },
  {
    title: 'Caméra',
    fields: [
      { kind: 'range', path: 'camera.followFraction', label: "Part de l'emprise visible", step: 0.005 },
      { kind: 'range', path: 'camera.stopSlowdown', label: 'Ralentissement aux lieux', step: 0.1 },
      { kind: 'range', path: 'camera.markerSlowdown', label: 'Ralentissement aux sprints et ascensions', step: 0.1 },
      { kind: 'range', path: 'camera.evenStages', label: 'Égaliser la durée des étapes', step: 0.05 },
      { kind: 'range', path: 'camera.zoom', label: 'Zoom du parcours (lissage inchangé)', step: 0.05 },
      { kind: 'range', path: 'camera.pitch', label: 'Inclinaison (0 : à plat)', step: 1 },
      { kind: 'range', path: 'camera.headingWindow', label: 'Lissage du cap, en écrans', step: 0.1 },
      { kind: 'range', path: 'camera.terrainExaggeration', label: 'Exagération du relief', step: 0.1 },
    ],
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

function toDraft(theme: Draft): Draft {
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

function control(field: Field, state: EditorState, draft: Draft, onChange: () => void): HTMLLabelElement {
  const label = document.createElement('label');
  const text = document.createElement('span');
  text.textContent = field.label;
  label.append(text);
  const current = valueAt(draft, field.path);

  if (field.kind === 'font' || field.kind === 'imagery') {
    const select = document.createElement('select');
    const options = field.kind === 'font' ? state.fonts : state.imageries;
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
  } else if (field.kind === 'text') {
    input.value = typeof current === 'string' ? current : '';
    input.size = 6;
    input.addEventListener('input', () => {
      setAt(draft, field.path, input.value);
      onChange();
    });
  } else if (field.kind === 'color') {
    input.value = typeof current === 'string' ? current : '#000000';
    input.addEventListener('input', () => {
      setAt(draft, field.path, input.value);
      onChange();
    });
  } else {
    const [min, max] = state.ranges[field.path] ?? [0, 1];
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

function renderControls(container: HTMLElement, state: EditorState, draft: Draft, onChange: () => void): void {
  container.replaceChildren(
    ...GROUPS.map((group) => {
      const fieldset = document.createElement('fieldset');
      const legend = document.createElement('legend');
      legend.textContent = group.title;
      fieldset.append(legend, ...group.fields.map((field) => control(field, state, draft, onChange)));
      return fieldset;
    }),
  );
}

/** Image `index` rendue avec le brouillon ; renvoie le message d'état (vide si tout va bien). */
async function showPreview(image: HTMLImageElement, draft: Draft, index: number): Promise<string> {
  const response = await fetch('/preview', { method: 'POST', body: JSON.stringify({ theme: draft, frame: index }) });
  if (response.status === 400) {
    const raw: unknown = await response.json();
    return Array.isArray(raw) ? describe(raw.filter(isIssue)) : 'Aperçu refusé';
  }
  if (!response.ok) return `Aperçu impossible : ${await response.text()}`;
  const previous = image.src;
  image.src = URL.createObjectURL(await response.blob());
  await image.decode().catch(() => undefined);
  if (previous.startsWith('blob:')) URL.revokeObjectURL(previous);
  return '';
}

const isIssue = (raw: unknown): raw is Issue =>
  isRecord(raw) && typeof raw['path'] === 'string' && typeof raw['expected'] === 'string';

function describe(issues: readonly Issue[]): string {
  return issues.map((issue) => `${issue.path} : ${issue.expected}`).join(' · ');
}

async function start(): Promise<void> {
  const response = await fetch('/editor.json');
  if (!response.ok) throw new EditorError('editor.json');
  const state: unknown = await response.json();
  if (!isEditorState(state)) throw new EditorError('editor.json');
  const status = element('status', HTMLDivElement);
  if (state.issues.length > 0)
    status.textContent = `Thème enregistré invalide, valeurs par défaut : ${describe(state.issues)}`;
  let draft = toDraft(state.theme);

  const { run } = state;
  const frame = element('preview', HTMLImageElement);
  const stage = element('stage', HTMLDivElement);
  Object.assign(frame, { width: String(run.width), height: String(run.height) });
  const fit = (): void => {
    const scale = Math.min(stage.clientWidth / run.width, stage.clientHeight / run.height);
    frame.style.transform = `translateX(-50%) scale(${String(scale)})`;
  };
  new ResizeObserver(fit).observe(stage);
  element('theme-name', HTMLDivElement).textContent = `Run ${run.name}`;

  const frames = run.frames;
  const slider = element('frame', HTMLInputElement);
  const frameLabel = element('frame-label', HTMLOutputElement);
  slider.max = String(frames - 1);
  slider.value = String(Math.round(frames / 3));

  // Un seul rendu à la fois ; un réglage arrivé pendant un rendu repart avec les dernières valeurs.
  let rendering = false;
  let pending = false;
  const refresh = async (): Promise<void> => {
    if (rendering) {
      pending = true;
      return;
    }
    rendering = true;
    frameLabel.textContent = `${slider.value} / ${slider.max}`;
    status.textContent = await showPreview(frame, draft, Number(slider.value));
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
  renderControls(controls, state, draft, onChange);
  slider.addEventListener('input', onChange);

  element('reset', HTMLButtonElement).addEventListener('click', () => {
    draft = toDraft(state.defaults);
    renderControls(controls, state, draft, onChange);
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
