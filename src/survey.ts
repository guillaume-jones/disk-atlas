import { NO_REGION, type Region, type SurveyMeta } from "./api";

const WORLD_ID = 0;

/** A finished scan: its totals and the region tree. */
export class Survey {
  constructor(readonly meta: SurveyMeta) {}

  region(id: number): Region {
    const region = this.meta.regions[id];
    if (!region) throw new RangeError(`Unknown region ${id}`);
    return region;
  }

  get world(): Region {
    return this.region(WORLD_ID);
  }

  /** Outermost first, excluding the world itself. */
  ancestors(id: number): Region[] {
    const chain: Region[] = [];
    for (let at = id; at !== NO_REGION && at !== WORLD_ID;) {
      const region = this.region(at);
      chain.unshift(region);
      at = region.parent;
    }
    return chain;
  }
}
