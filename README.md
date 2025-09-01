# Documentation
see https://github.com/random998/bachelors_project/blob/main/docs/report.md for extensive documentation of the project.

# Prerequisites
1. Install Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` (or via rustup.rs).
2. Use stable: `rustup default stable`.
3. On Linux (for egui): `sudo apt install libclang-dev libgtk-3-dev libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev libspeechd-dev libxkbcommon-dev libssl-dev`\
   (or equivalent on Fedora: `sudo dnf install clang-devel gtk3-devel` ... – see egui docs).\

# Build & Run
1. Clone: `git clone https://github.com/random998/bachelors_project && cd bachelors_project`
2. Build: `cargo build --release`
3. Run GUI: 1cargo run --release --bin gui (starts the egui app; connect peers manually via CLI args if needed, e.g., --seed-peer /ip4/127.0.0.1/tcp/1234/p2p/<peer_id> – check src/gui/main.rs for options).`
4. Test: `cargo test` (runs unit/integration; expect some failures on distributed – WIP).
5. Debug: `RUST_LOG=info cargo run --release --bin gui` for logs.
