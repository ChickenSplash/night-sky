# Night Sky

A space simulator built in Rust around scientific plausibility, running natively on Linux and in a browser. Explore a procedurally generated galaxy at real distances, with orbiting planets, multiple-star systems and physical lighting.

Both native Linux and browser clients are supported, with neither the primary target. They share the simulation and interface, with some platform-specific capabilities.

Highly optimised star generation, streaming and GPU rendering make galaxy-scale exploration possible. The project is actively developed, with plans to simulate more physical phenomena and improve visual fidelity. The galaxy is synthetic, grounded in real-world observations and scientific research.

## Run natively

Requires Rust via [rustup](https://rustup.rs/), Bash, curl, Clang, LLD, pkg-config, Wayland/X11 development libraries and a Vulkan-capable graphics driver. The Rust toolchain is pinned in `rust-toolchain.toml`.

```sh
git clone https://github.com/ChickenSplash/night-sky.git
cd night-sky
./scripts/run.sh
```

The first build downloads and compiles dependencies. Closing the application also stops its local server.

While the native client runs, `./scripts/ns` is a CLI that can read and drive all of the application's state, commands and resources (`./scripts/ns help` lists them):

```sh
./scripts/ns state camera
./scripts/ns lists
./scripts/ns cmd --wait "jump demo-flight"
./scripts/ns screenshot
./scripts/ns search kind=planet planet_class=sub_neptune orbits.star_class=m within_ly=50
```

If you are on any other platform outside of Linux, just chuck this to your AI of choice, I'm sure they can figure it out.

## Run in a browser

The build script currently targets x86-64 Linux. It requires Rust via rustup, Clang, LLD, Python 3 (available as `python`), curl, tar and gzip.

```sh
./scripts/build-web.sh
cargo run --locked -p space-server
```

Open **http://127.0.0.1:4321/** in a desktop browser with WebGL 2 and hardware acceleration enabled. The server serves the browser build and demo data; rendering happens on your device. Stop the server with Ctrl+C.

## For developers and agents

See the [documentation](docs/README.md) for controls, development and explanations of the simulation, rendering and interface. [AGENTS.md](AGENTS.md) provides a code-to-docs map and working conventions.
