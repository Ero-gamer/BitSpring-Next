// The backdrop's animatable state, kept apart from scene.ts so the page can set up its scroll
// timeline without waiting for three.js, which loads as its own chunk.

/** Everything the scroll timeline animates. Plain numbers so GSAP can tween them directly. */
export interface FieldState {
  camX: number;
  camY: number;
  camZ: number;
  lookX: number;
  lookY: number;
  lookZ: number;
  /** 0 = untouched signal (brass), 1 = processed by enhancers (violet). */
  process: number;
  /** 0 = whole field, 1 = only the centre row (a single clean stem plot). */
  focus: number;
  /** Samples per wave cycle; higher = higher sample rate. */
  density: number;
  /** Quantisation levels per half-scale; 1 = one bit (DSD). */
  levels: number;
  amp: number;
  /** Overall brightness, used to dim the field behind dense content. */
  visible: number;
}

export const HERO: FieldState = {
  camX: -7.5,
  camY: 2.4,
  camZ: 10.5,
  lookX: 2.5,
  lookY: -1.3,
  lookZ: 0,
  process: 0,
  focus: 0,
  density: 1,
  levels: 7,
  amp: 1,
  visible: 1,
};
