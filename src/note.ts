import type { Region } from "./api";
import { element } from "./dom";
import { formatAge, formatBytes, homeRelative } from "./format";
import type { Survey } from "./survey";

/** The ruled note in the viewer's hand, describing the hovered region or the whole survey. */
export class Note {
  private readonly root = element("note", HTMLElement);
  private readonly name = element("note-name", HTMLDivElement);
  private readonly path = element("note-path", HTMLDivElement);
  private readonly facts = element("note-facts", HTMLDivElement);

  show(survey: Survey, region: Region, home: string) {
    const { meta } = survey;
    const isWorld = region.kind === "world";
    const size = isWorld
      ? `${formatBytes(meta.scanned)} surveyed, ${formatBytes(meta.free)} free`
      : formatBytes(region.size);
    this.root.hidden = false;
    this.name.textContent = region.name;
    this.path.textContent = region.path ? homeRelative(region.path, home) : region.label;
    this.facts.textContent =
      region.mtime && !isWorld ? `${size} · ${formatAge(region.mtime, meta.now)}` : size;
  }
}
