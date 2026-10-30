import type { Theme } from "../theme";
import { edwardian } from "./edwardian";
import { modern } from "./modern";

export const THEMES: readonly Theme[] = [edwardian, modern];
export const DEFAULT_THEME = edwardian;
