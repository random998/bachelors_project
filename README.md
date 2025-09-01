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
4. execute the gui executable: (`RUST_LOG=info ./poker_gui`): A Window with the title `zk-poker` should open.
It is recommended to set the logging level to info, to observe what is happening behind the scenes (`RUST_LOG=info`).
6. enter a nickname and press 'Connect'.
![login_view](https://github.com/random998/bachelors_project/blob/adf19b6e4c687846ba1c43eb2c1a6560e258f060/docs/connect_view.png)
7. In the 'Account view', press 'Join Table'.
![account_view](https://github.com/random998/bachelors_project/blob/adf19b6e4c687846ba1c43eb2c1a6560e258f060/docs/account_view.png)
8. Wait a couple of seconds, then on the bottom right of the screen, an address occurs:
![game_view](https://github.com/random998/bachelors_project/blob/adf19b6e4c687846ba1c43eb2c1a6560e258f060/docs/game_view_peer_address.png)
9. In a new commandline window, preferrably, navigate again into the build directory (`cd target/debug`). Then enter the following command:
   `RUST_LOG=info ./poker_gui --seed-addr /ip4/192.168.2.153/tcp/36475` where the seed-address is exactly the address you observed in step 7.
10. Now a new window, showing the login_view as in step 7, should appear.
11. The console, in which you started the second application (step 9), the console should print (if you ran the command in step 9 with `RUST_LOG=info`)
    something in the lines of
```
$ RUST_LOG=info ./poker_gui --seed-addr /ip4/192.168.2.153/tcp/34643
[2025-09-01T11:17:48Z INFO  p2p_net::swarm_task] successfully dialed seed peer at /ip4/192.168.2.153/tcp/34643
[2025-09-01T11:17:48Z INFO  p2p_net::swarm_task] 12D3KooWNHbVGcpStZnkxGqHJHEHRY6T8tHD3wKMsuD3rMUZW99K subscribed to poker/1
[2025-09-01T11:17:48Z INFO  poker_core::game_state] peer 12D3KooWNHbVGcpStZnkxGqHJHEHRY6T8tHD3wKMsuD3rMUZW99K has nickname default_nick
```
12. Note that `successfully dialed seed peer at /ip4/192.168.2.153/tcp/34643` indicates that we succesfully established a connection to our first peer (the applicatoin which we started in step 4).
13. Now repeat steps 6 to 8 for the second peer and two players should have joined both gui instances:
![two_peers](https://github.com/random998/bachelors_project/blob/b44281c4fcd13598950b9f1e9060cf2171687555/docs/two_peers_game_view.png)
14. Repeat steps 9 to 13 for a third player (peer) to join the game, and for all three gui instances, 3 players should be visible at the table, and the game should have started.
![3_player_game_start](https://github.com/random998/bachelors_project/blob/e206008a49f6e69c88a1c50553992faa8499ea0f/docs/3_players_game_view.png)
