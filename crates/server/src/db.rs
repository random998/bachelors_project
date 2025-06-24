// code inspired by https://github.com/vincev/freezeout
//! Database types and functions for persisting game state.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};
use parking_lot::Mutex;
use poker_core::crypto::PeerId;
use poker_core::poker::Chips;
use rusqlite::{params, Connection};

/// A single row in the `players` table.
#[derive(Debug,)]
pub struct Player {
    pub player_id: PeerId,
    pub nickname: String,
    pub chips: Chips,
}

/// Persistent database for storing player state.
#[derive(Debug, Clone,)]
pub struct Database {
    connection: Arc<Mutex<Connection,>,>,
}

impl Database {
    /// Open a new database file on disk.
    pub fn open<P: AsRef<Path,>,>(path: P,) -> Result<Self,> {
        let conn = Connection::open(path,)?;
        Self::initialize_schema(&conn,)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(conn,),),
        },)
    }

    /// Open a temporary in-memory database.
    pub fn open_in_memory() -> Result<Self,> {
        let conn = Connection::open_in_memory()?;
        Self::initialize_schema(&conn,)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(conn,),),
        },)
    }

    fn initialize_schema(conn: &Connection,) -> Result<(),> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;",)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS players (
            id TEXT PRIMARY KEY,
            nickname TEXT NOT NULL,
            chips INTEGER NOT NULL,
            created_at TEXT DEFAULT CURRENT_TIMESTAMP CHECK (
                created_at GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9] \
             [0-2][0-9]:[0-5][0-9]:[0-5][0-9]'
            ),
            last_update TEXT DEFAULT CURRENT_TIMESTAMP CHECK (
                last_update GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]-[0-3][0-9] \
             [0-2][0-9]:[0-5][0-9]:[0-5][0-9]'
            )
        ) STRICT;",
            (),
        )?;
        Ok((),)
    }

    /// Insert or update a player joining the server.
    pub async fn upsert_player(
        &self, player_id: PeerId, nickname: &str, initial_chips: Chips,
    ) -> Result<Player,> {
        let db = self.connection.clone();
        let nickname = nickname.to_string();

        tokio::task::spawn_blocking(move || {
            let conn = db.lock();

            let mut stmt = conn.prepare("SELECT id, nickname, chips FROM players WHERE id = ?1",)?;
            let query_result = stmt.query_row(params![player_id.digits().to_string()], |row| {
                Ok(Player {
                    player_id,
                    nickname: row.get(1,)?,
                    chips: Chips::new(row.get::<usize, i32>(0,)? as u32,),
                },)
            },);

            match query_result {
                | Ok(mut player,) => {
                    let mut updated = false;

                    if player.chips < initial_chips {
                        player.chips = initial_chips;
                        updated = true;
                    }

                    if player.nickname != nickname {
                        player.nickname = nickname.clone();
                        updated = true;
                    }

                    if updated {
                        conn.execute(
                            "UPDATE players SET chips = ?2, nickname = ?3, last_update = \
                             CURRENT_TIMESTAMP WHERE id = ?1",
                            params![
                                player.player_id.digits(),
                                player.chips.amount(),
                                player.nickname
                            ],
                        )?;
                    }

                    Ok(player,)
                },
                | Err(_,) => {
                    let new_player = Player {
                        player_id,
                        nickname: nickname.clone(),
                        chips: initial_chips,
                    };
                    conn.execute(
                        "INSERT INTO players (id, nickname, chips)VALUES (?1, ?2, ?3)ON \
                         CONFLICT(id) DO UPDATE SET nickname = excluded.nickname,
                             chips = excluded.chips,
                             created_at = excluded.created_at;
                             ",
                        params![
                            player_id.digits().to_string(),
                            nickname.to_string(),
                            initial_chips.amount()
                        ],
                    )?;
                    Ok(new_player,)
                },
            }
        },)
        .await?
    }

    /// Attempt to deduct chips from a player. Returns `false` if player has
    /// insufficient chips.
    pub async fn deduct_chips(&self, player_id: PeerId, amount: Chips,) -> Result<bool,> {
        let db = self.connection.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.lock();

            let mut stmt = conn.prepare("SELECT chips FROM players WHERE id = ?1",)?;
            let result = stmt.query_row(params![player_id.digits()], |row| {
                Ok(Chips::new(row.get::<usize, i32>(0,)? as u32,),)
            },);

            match result {
                | Ok(current_chips,) => {
                    if current_chips < amount {
                        return Ok(false,);
                    }
                    let remaining = current_chips - amount;
                    conn.execute(
                        "UPDATE players SET chips = ?2, last_update = CURRENT_TIMESTAMP WHERE id \
                         = ?1",
                        params![player_id.digits(), remaining.amount()],
                    )?;
                    Ok(true,)
                },
                | Err(e,) => Err(e.into(),),
            }
        },)
        .await?
    }

    /// Credit chips to a player.
    pub async fn credit_chips(&self, player_id: PeerId, amount: Chips,) -> Result<(),> {
        let db = self.connection.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.lock();
            let digits = player_id.digits().to_string();
            println!("credit chips, player id digits: {}", digits);

            let print_rows = conn.execute("SELECT id, chips FROM players;", params![],);

            let rows_updated = conn.execute(
                "UPDATE players SET chips = chips + ?2, last_update = CURRENT_TIMESTAMP WHERE id \
                 = ?1",
                params![digits, amount.amount()],
            )?;

            if rows_updated == 0 {
                bail!("Player {} not found", player_id.digits().to_string());
            }

            Ok((),)
        },)
        .await?
    }

    /// Fetch a player by ID.
    pub async fn get_player_by_id(&self, player_id: PeerId,) -> Result<Player,> {
        let db = self.connection.clone();

        tokio::task::spawn_blocking(move || {
            let conn = db.lock();

            let mut stmt = conn.prepare("SELECT id, nickname, chips FROM players WHERE id = ?1",)?;
            stmt.query_row(params![player_id.digits()], |row| {
                Ok(Player {
                    player_id,
                    nickname: row.get(1,)?,
                    chips: {
                        let chips: i64 = row.get(2,)?;
                        let chips = Chips::new(chips as u32,);
                        chips
                    },
                },)
            },)
                .map_err(anyhow::Error::from,)
        },)
        .await?
    }
}

#[cfg(test)]
mod tests {
    use poker_core::crypto::SigningKey;

    use super::*;

    #[tokio::test]
    async fn test_player_lifecycle() {
        let db = Database::open_in_memory().expect("Failed to open in-memory database",);
        let player_id = SigningKey::default().verifying_key().peer_id();
        let chips = Chips::new(1_000,);

        let player = db.upsert_player(player_id, "Alice", chips,).await.expect("Join failed",);
        assert_eq!(player.nickname, "Alice");
        assert_eq!(player.chips, chips);

        let updated_player =
            db.upsert_player(player_id, "Bob", chips * 2,).await.expect("Update failed",);
        assert_eq!(updated_player.nickname, "Bob");
        assert_eq!(updated_player.chips, chips * 2);

        db.credit_chips(player_id, chips,).await.expect("Credit failed",);
        let post_credit = db.get_player_by_id(player_id,).await.expect("Fetch failed",);
        assert_eq!(post_credit.chips, chips * 3);

        let can_deduct = db.deduct_chips(player_id, chips,).await.expect("Deduct failed",);
        assert!(can_deduct);
        let post_deduct = db.get_player_by_id(player_id,).await.expect("Fetch failed",);
        assert_eq!(post_deduct.chips, chips * 2);

        let can_deduct = db.deduct_chips(player_id, chips * 2,).await.expect("Deduct failed",);
        assert!(can_deduct);
        let zero_balance = db.get_player_by_id(player_id,).await.expect("Fetch failed",);
        assert_eq!(zero_balance.chips, Chips::new(0));

        let cannot_deduct = db.deduct_chips(player_id, chips,).await.expect("Deduct failed",);
        assert!(!cannot_deduct);
    }
}
