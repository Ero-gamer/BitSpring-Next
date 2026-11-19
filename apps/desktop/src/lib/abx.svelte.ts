// The blind test (ABX): asked for in one place (the quality dialog, the enhancers), shown by
// the page, which knows the output it plays on.

export type AbxRequest =
  /** Two files of the same music. */
  | { kind: "files"; a: string; b: string; title: string; aName: string; bName: string }
  /** One file as it is (A) against the same file through the processing (B). */
  | { kind: "processing"; a: string; title: string };

export const abx = $state({ request: null as AbxRequest | null });

export function startAbx(request: AbxRequest) {
  abx.request = request;
}
