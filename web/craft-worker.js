// Full bodies only for a streamed star the craft is actually approaching.
import init, { craft_system } from './sim_worker.js';

const early = [];
let ready = false;
function build(text) {
  const [token, id, position] = JSON.parse(text);
  try {
    const system = JSON.parse(craft_system(id, ...position));
    self.postMessage(JSON.stringify([token, system]));
  } catch {
    self.postMessage(JSON.stringify([token, null]));
  }
}
self.onmessage = ({ data }) => {
  if (ready) build(data);
  else early.push(data);
};
await init();
ready = true;
early.splice(0).forEach(build);
