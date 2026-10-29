import * as api from "./api";
import type { Roots, Status } from "./api";
import { element } from "./dom";
import { formatBytes, homeRelative } from "./format";

const POLL_MS = 250;

/** The title page: choosing what to survey, and following the scan until the map is ready. */
export class TitlePage {
  private readonly page = element("start", HTMLElement);
  private readonly closeButton = element("close-start", HTMLButtonElement);
  private readonly homePath = element("home-path", HTMLSpanElement);
  private readonly diskPath = element("disk-path", HTMLSpanElement);
  private readonly customPath = element("custom-path", HTMLInputElement);
  private readonly progress = element("progress", HTMLParagraphElement);
  private readonly error = element("error", HTMLParagraphElement);
  private readonly access = element("access", HTMLDivElement);
  private roots: Roots = { home: "", disk: "" };
  private pendingRoot = "";

  constructor(private readonly onReady: () => Promise<void>) {
    for (const button of this.page.querySelectorAll<HTMLButtonElement>("button[data-root]")) {
      button.addEventListener("click", () =>
        this.scan(button.dataset.root === "disk" ? this.roots.disk : this.roots.home),
      );
    }
    element("custom", HTMLFormElement).addEventListener("submit", (event) => {
      event.preventDefault();
      const path = this.customPath.value.trim();
      if (path) this.scan(path.startsWith("~") ? this.roots.home + path.slice(1) : path);
    });
    this.closeButton.addEventListener("click", () => this.close());
    element("access-open", HTMLButtonElement).addEventListener("click", () => {
      api.openFullDiskAccessSettings().catch((error: unknown) => this.showError(error));
    });
    element("access-skip", HTMLButtonElement).addEventListener("click", () =>
      this.scan(this.pendingRoot, true),
    );
  }

  get home(): string {
    return this.roots.home;
  }

  /** Shows the scan roots, then picks up whatever the engine is already doing. */
  async start() {
    try {
      this.roots = await api.roots();
      this.homePath.textContent = homeRelative(this.roots.home, this.roots.home);
      this.diskPath.textContent = this.roots.disk;
      await this.follow(await api.status());
    } catch (error) {
      this.showError(error);
    }
  }

  open(closable: boolean) {
    this.page.hidden = false;
    this.closeButton.hidden = !closable;
  }

  private close() {
    this.page.hidden = true;
  }

  private async scan(root: string, skipAccessCheck = false) {
    this.error.hidden = true;
    this.access.hidden = true;
    this.setChoicesEnabled(false);
    try {
      if (!skipAccessCheck && (await api.needsFullDiskAccess(root))) {
        this.pendingRoot = root;
        this.access.hidden = false;
        this.setChoicesEnabled(true);
        return;
      }
      await api.startScan(root);
      await this.follow(await api.status());
    } catch (error) {
      this.showError(error);
    }
  }

  private async follow(status: Status) {
    switch (status.phase) {
      case "idle":
        return;
      case "scanning":
      case "mapping": {
        this.setChoicesEnabled(false);
        const verb = status.phase === "mapping" ? "Drawing the map" : "Surveying";
        this.progress.hidden = false;
        this.progress.textContent = `${verb}… ${status.files.toLocaleString()} files, ${formatBytes(status.bytes)}`;
        setTimeout(() => {
          api
            .status()
            .then((next) => this.follow(next))
            .catch((error: unknown) => this.showError(error));
        }, POLL_MS);
        return;
      }
      case "ready":
        await this.onReady();
        this.close();
        this.progress.hidden = true;
        this.setChoicesEnabled(true);
        return;
      case "failed":
        this.showError(status.message);
        return;
    }
  }

  private showError(error: unknown) {
    this.progress.hidden = true;
    this.error.hidden = false;
    this.error.textContent = String(error);
    this.setChoicesEnabled(true);
  }

  private setChoicesEnabled(enabled: boolean) {
    for (const button of this.page.querySelectorAll("button")) button.disabled = !enabled;
  }
}
