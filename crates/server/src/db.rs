// code inspired by https://github.com/vincev/freezeout

//! Database types for persisting state.
use anyhow::{Result, bail};
use parking_lot::Mutex;
use rusqlite::{Connection, params};
use std::{path::Path, sync::Arc};

use zkpoker::{crypto::PeerId, poker::Chips};

/// A database player row.
#[derive(Debug)]
pub struct Player {
    pub player_id: PeerId,
    pub nickname: String,
    pub chips: Chips,
}

/// Database for persisting the game state.
#[derive(Debug, Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open a database at the specified path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;

        Self::init_database(Mutex::new(conn))?;

        Ok(Db {
            conn: Arc::new(Mutex::new(conn))
        })
    }

    /// Open an in-memory database.
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::init_database(Mutex::new(conn))?;
        Ok(Db {
            conn: Arc::new(Mutex::new(conn))
        })
    }

    fn init_database(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous=NORMAL;")?; //TODO: ????

        // create tables.
        conn.execute(
            "CREATE TABLE IF NOT EXISTS players\
            (\
                id TEXT PRIMARY KEY, \
                nickname TEXT NOT NULL, \
                chips INTEGER NOT NULL, \
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP \
                last_update DATETIME DEFAULT CURRENT_TIMESTAMP
            )",
            (),
        )?;
        Ok(())
    }


    /// A player joined the server.
    ///
    /// If the player doesn't exist it creates one with the given chips, for now if
    /// the player exists but has fewer chips than join chips the chips are updated
    /// so that the player has enough chips to join. TODO: fix
}