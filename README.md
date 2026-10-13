# Night Sky

A realistic space simulator built in Rust with AI assistance, running natively on Linux and in a browser. Explore a procedurally generated galaxy at real distances, with orbiting planets, multiple-star systems and physical lighting.

Highly optimised star generation, streaming and GPU rendering make galaxy-scale exploration possible. The project is actively developed, with plans to simulate more physical phenomena and improve visual fidelity. The galaxy is synthetic, grounded in real-world observations and scientific research rather than an astronomical catalogue.

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

Commands that would do nothing, such as an unknown ID or an out-of-range value, are refused with the reason and a nonzero exit code.

## Run in a browser

The build script currently targets x86-64 Linux. It requires Rust via rustup, Clang, LLD, Python 3 (available as `python`), curl, tar and gzip.

```sh
./scripts/build-web.sh
cargo run --locked -p space-server
```

Open **http://127.0.0.1:4321/** in a desktop browser with WebGL 2 and hardware acceleration enabled. The server serves the browser build and demo data; rendering happens on your device. Stop the server with Ctrl+C.

## For developers and agents

See the [documentation](docs/README.md) for controls, project structure, development checks and simulation/rendering design. Contribution rules and invariants live in [AGENTS.md](AGENTS.md).
