use std::sync::mpsc::{Receiver, Sender};
use anyhow::Error;

use poker_core::message::SignedMessage;
use poker_core::net::{NetRx, NetTx};

// mock-transport.rs TODO!