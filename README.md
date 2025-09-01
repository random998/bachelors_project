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

# Running the Application
1. Navigate to the main repository (`bachelors_project`).
2. enter `cargo build` into the command line.
3. navigate to the build directory (`cd target/debug`).
4. execute the gui executable: (`./poker_gui`): A Window with the title `zk-poker` should open.
5. enter a nickname and press 'Connect'.
![connect_view](https://github.com/random998/bachelors_project/blob/adf19b6e4c687846ba1c43eb2c1a6560e258f060/docs/connect_view.png)
6. In the 'Account view', press 'Join Table'.
![account_view](https://github.com/random998/bachelors_project/blob/adf19b6e4c687846ba1c43eb2c1a6560e258f060/docs/account_view.png)
8. Wait a couple of seconds, then on the bottom right of the screen, an address occurs:
![game_view](https://github.com/random998/bachelors_project/blob/adf19b6e4c687846ba1c43eb2c1a6560e258f060/docs/game_view_peer_address.png)
