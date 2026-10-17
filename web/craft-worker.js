// What craft wait for: the systems near a region, and one system's bodies.
import init, { craft_answer } from './sim_worker.js';

const early = [];
let ready = false;
function answer(text) {
  const [token, request] = JSON.parse(text);
  try {
    self.postMessage(JSON.stringify([token, JSON.parse(craft_answer(JSON.stringify(request)))]));
  } catch {
    self.postMessage(JSON.stringify([token, null]));
  }
}
self.onmessage = ({ data }) => {
  if (ready) answer(data);
  else early.push(data);
};
await init();
ready = true;
early.splice(0).forEach(answer);
