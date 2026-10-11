// Dedicated worker: star generation has its own WASM memory and event loop.
import init, { StarWorker } from './star_worker.js';

let loader;
let latest;
self.onmessage = ({ data }) => {
  latest = data;
  if (loader) aim();
};
await init();
// The client starts a new worker for each magnitude limit (`?magnitude=`).
loader = new StarWorker(Number(new URL(self.location.href).searchParams.get('magnitude') ?? NaN));
let scheduled = false;
if (latest) aim();
function aim() {
  loader.aim(...latest);
  schedule();
}
function run() {
  scheduled = false;
  const packet = loader.step();
  self.postMessage(packet.buffer, [packet.buffer]);
  if (loader.pending()) schedule();
}
function schedule() {
  if (!scheduled) {
    scheduled = true;
    setTimeout(run, 0);
  }
}
