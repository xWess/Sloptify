// Sloptify plugin template — drop this folder into:
//   ~/.local/share/sloptify/plugins/example.hello/
// then Enable it from Account → Plugin marketplace.
//
// Host JS sandbox (rquickjs) is next; today Sloptify validates the package
// and persists enablement so contributors can ship the shape early.

export const id = "example.hello";

export async function onTrackChanged(track) {
  // await host.notify(`${track.name} — ${track.artist}`)
  console.debug("[example.hello]", track?.name);
}
