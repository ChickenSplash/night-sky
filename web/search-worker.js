// Dedicated worker: searches have their own WASM instance and event loop,
// apart from the star worker, which restarts on every magnitude change.
import init, { SearchWorker } from './sim_worker.js';

const early = [];
let runner;
self.onmessage = ({ data }) => {
  if (runner) order(data);
  else early.push(data);
};
await init();
runner = new SearchWorker();
early.splice(0).forEach(order);
let scheduled = false;
function order(json) {
  const update = runner.order(json);
  if (update !== undefined) self.postMessage(update);
  schedule();
}
function run() {
  scheduled = false;
  const update = runner.step();
  if (update !== undefined) self.postMessage(update);
  if (runner.busy()) schedule();
}
function schedule() {
  if (!scheduled && runner.busy()) {
    scheduled = true;
    setTimeout(run, 0);
  }
}
