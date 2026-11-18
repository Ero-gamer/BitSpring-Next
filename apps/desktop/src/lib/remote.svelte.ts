// The phone remote (computers only: a phone is the remote), shared by the two places that
// show it: Settings switches it on, the header keeps its code at hand while it is on.

import { api, isAndroid, isIOS, type RemoteStatus } from "./api";

export const hasRemote = !isIOS && !isAndroid;

export const remote = $state({
  status: null as RemoteStatus | null,
  /** Switched on a moment ago, in Settings: the code is shown large there, to scan now. */
  fresh: false,
});

export async function loadRemote() {
  if (!hasRemote) return;
  try {
    remote.status = await api.remoteStatus();
  } catch {
    // No answer: the remote stays hidden.
  }
}

export async function setRemote(on: boolean) {
  remote.status = await api.remoteEnable(on);
  remote.fresh = on && !!remote.status.url;
}

export async function newAddress() {
  remote.status = await api.remoteNewAddress();
}
