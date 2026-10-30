import * as api from "./api";
import type { Region } from "./api";
import { Camera } from "./camera";
import { element } from "./dom";
import type { Point } from "./geometry";
import { Ink } from "./ink";
import { bindInput } from "./input";
import { Layers } from "./layers";
import { nameRegions } from "./names";
import { Note } from "./note";
import { Sheet } from "./sheet";
import { Survey } from "./survey";
import { Terrain } from "./terrain";
import type { Theme } from "./theme";
import { DEFAULT_THEME, THEMES } from "./themes";
import { TitlePage } from "./title";
import { woodTexture } from "./wood";

/** Patch requests wait for the view to rest this long, so gestures don't fetch every intermediate view. */
const REFINE_DELAY_MS = 120;
const THEME_KEY = "disk-atlas-theme";
const THEME_FONT_STYLES = ["normal", "italic", "700"];

interface Session {
  survey: Survey;
  layers: Layers;
}

class Atlas {
  readonly camera = new Camera(() => this.viewChanged());
  session: Session | null = null;
  private readonly terrainCanvas = element("terrain", HTMLCanvasElement);
  private readonly inkCanvas = element("ink", HTMLCanvasElement);
  private readonly toast = element("toast", HTMLDivElement);
  private readonly tickets = element("tickets", HTMLDivElement);
  private readonly themeButtons = element("themes", HTMLElement);
  private readonly terrain = new Terrain(this.terrainCanvas);
  private readonly ink = new Ink(this.inkCanvas);
  private readonly sheet = new Sheet();
  private readonly note = new Note();
  private readonly title = new TitlePage(() => this.loadSurvey());
  private readonly woods = new Map<string, string>();
  private theme = DEFAULT_THEME;
  private dirty = true;
  private resting = true;
  private refineTimer = 0;

  constructor() {
    this.themeButtons.replaceChildren(
      ...THEMES.map((theme) => {
        const button = document.createElement("button");
        button.textContent = theme.label;
        button.dataset.theme = theme.id;
        button.addEventListener("click", () => this.applyTheme(theme));
        return button;
      }),
    );
    const saved = localStorage.getItem(THEME_KEY);
    this.applyTheme(THEMES.find(({ id }) => id === saved) ?? DEFAULT_THEME);
    bindInput(this.inkCanvas, this.camera, {
      isActive: () => this.session !== null,
      hover: (point) => this.hover(point),
      click: (point, reveal) => this.click(point, reveal),
    });
    element("rescan", HTMLButtonElement).addEventListener("click", () =>
      this.title.open(this.session !== null),
    );
    window.addEventListener("resize", () => this.resize());
    this.resize();
    requestAnimationFrame(this.frame);
    void this.title.start();
  }

  private readonly frame = () => {
    requestAnimationFrame(this.frame);
    if (!this.dirty) return;
    this.dirty = false;
    const { camera, session } = this;
    const visible = session?.layers.visible(camera.visibleWorld()) ?? [];
    const meshed = session?.layers.meshed(visible, 1 / (camera.scale * camera.dpr)) ?? [];
    this.terrain.draw(
      meshed.map((layer) => layer.gl),
      camera,
      camera.mapOnScreen(),
      this.resting,
    );
    if (session) this.ink.draw(camera, session.survey, session.layers, visible, this.theme);
    else this.ink.clear(camera);
    this.sheet.place(camera, session !== null);
  };

  private viewChanged() {
    this.dirty = true;
    this.resting = false;
    clearTimeout(this.refineTimer);
    this.refineTimer = window.setTimeout(() => {
      this.resting = true;
      this.dirty = true;
      if (!this.session) return;
      this.session.layers.refine();
      this.sheet.updateScale(this.camera, this.session.survey.meta.capacity);
    }, REFINE_DELAY_MS);
  }

  private resize() {
    const dpr = window.devicePixelRatio || 1;
    for (const canvas of [this.terrainCanvas, this.inkCanvas]) {
      canvas.width = Math.round(window.innerWidth * dpr);
      canvas.height = Math.round(window.innerHeight * dpr);
    }
    this.camera.resize(window.innerWidth, window.innerHeight, dpr);
  }

  private applyTheme(theme: Theme) {
    this.theme = theme;
    localStorage.setItem(THEME_KEY, theme.id);
    this.terrain.applyTheme(theme.terrain);
    this.sheet.applyTheme(theme);
    const root = document.documentElement.style;
    for (const [name, value] of Object.entries(theme.ui)) root.setProperty(`--${name}`, value);
    const wood = this.woods.get(theme.id) ?? woodTexture(theme.table);
    this.woods.set(theme.id, wood);
    document.body.style.backgroundImage = `url(${wood})`;
    for (const button of this.themeButtons.querySelectorAll<HTMLButtonElement>("button"))
      button.setAttribute("aria-pressed", String(button.dataset.theme === theme.id));
    // The canvas doesn't trigger web font loading itself.
    const fonts = THEME_FONT_STYLES.map((style) =>
      document.fonts.load(`${style} 16px ${theme.ui["text-font"]}`),
    );
    void Promise.all(fonts).then(() => {
      this.dirty = true;
    });
    if (this.session) {
      const { survey } = this.session;
      nameRegions(survey.meta.regions, theme.vocabulary);
      this.note.show(survey, survey.world, this.title.home);
    }
    this.dirty = true;
  }

  private async loadSurvey() {
    const { survey: meta, patch } = await api.world();
    this.session?.layers.dispose();
    const survey = new Survey(meta);
    nameRegions(survey.meta.regions, this.theme.vocabulary);
    const layers = new Layers(survey, patch, this.terrain, this.camera, {
      onChange: () => {
        this.dirty = true;
      },
      onBusy: (busy) => {
        this.toast.hidden = !busy;
      },
    });
    this.session = { survey, layers };
    this.tickets.hidden = false;
    this.note.show(survey, survey.world, this.title.home);
    this.camera.reset();
  }

  private regionAt(session: Session, point: Point): Region | null {
    const at = this.camera.toWorld(point.x, point.y);
    const hit = session.layers.probe(at.x, at.y);
    return hit ? session.survey.region(hit.region) : null;
  }

  private hover(point: Point | null) {
    if (!this.session) return;
    const { survey } = this.session;
    const region = point ? this.regionAt(this.session, point) : null;
    this.note.show(survey, region ?? survey.world, this.title.home);
  }

  /** Zooms to the outermost enclosing region that is still small on screen, else magnifies around the click. */
  private click(point: Point, reveal: boolean) {
    if (!this.session) return;
    const { survey, layers } = this.session;
    const target = this.regionAt(this.session, point);
    if (!target) return;
    if (reveal) {
      api.reveal(target.path).catch(console.error);
      return;
    }
    for (const region of survey.ancestors(target.id)) {
      if (region.kind === "ocean") return;
      const label = layers.labels.get(region.id)?.item;
      if (label && this.camera.isSmall(label.bounds)) {
        this.camera.focusOn(label.bounds);
        return;
      }
    }
    const anchor = this.camera.toWorld(point.x, point.y);
    this.camera.animateTo(anchor.x, anchor.y, this.camera.scale * 2);
  }
}

const atlas = new Atlas();
if (import.meta.env.DEV) Object.assign(window, { atlas });
