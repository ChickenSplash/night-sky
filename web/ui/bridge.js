// Connects the HTML panels to the Rust client. Commands sent before the
// WebAssembly module starts are held until `connect` supplies its entry point.
export function createBridge() {
  let send = null;
  const pending = [];
  const listeners = { galaxy: [], state: [], tick: [] };
  const latest = {};
  for (const name of Object.keys(listeners)) {
    window.addEventListener(`space:${name}`, event => {
      latest[name] = JSON.parse(event.detail);
      for (const listener of listeners[name]) listener(latest[name]);
    });
  }
  const deliver = command => {
    try {
      send(JSON.stringify(command));
    } catch (error) {
      console.error('Rejected UI command', command, error);
    }
  };
  return {
    connect(sendCommand) {
      send = sendCommand;
      pending.splice(0).forEach(deliver);
    },
    command(command) {
      if (send) deliver(command); else pending.push(command);
    },
    on(name, listener) {
      listeners[name].push(listener);
      if (latest[name]) listener(latest[name]);
    },
  };
}
