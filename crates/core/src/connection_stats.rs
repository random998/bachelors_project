// crates/core/src/connection_stats.rs
use std::time::{Duration, Instant};

#[derive(Debug, Clone,)]
pub struct ConnectionStats {
    started_at: Instant,
    pub peers:  usize, // currently connected peers

    // traffic counters
    pub sent_msgs: u64,
    pub recv_msgs: u64,
    pub dropped:   u64,

    // latency sampling
    last_ping_rtt: Option<Duration,>,
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self {
            started_at:    Instant::now(),
            peers:         0,
            sent_msgs:     0,
            recv_msgs:     0,
            dropped:       0,
            last_ping_rtt: None,
        }
    }
}

impl ConnectionStats {
    // ---------- public helpers called from your networking layer --------

    pub const fn inc_sent(&mut self,) {
        self.sent_msgs += 1;
    }
    pub const fn inc_recv(&mut self,) {
        self.recv_msgs += 1;
    }
    pub const fn inc_dropped(&mut self,) {
        self.dropped += 1;
    }
    pub const fn set_peers(&mut self, n: usize,) {
        self.peers = n;
    }
    pub const fn set_rtt(&mut self, rtt: Duration,) {
        self.last_ping_rtt = Some(rtt,);
    }

    // ---------- getters for the UI / logs -------------------------------

    /// Seconds since this node was started.
    pub fn uptime(&self,) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn last_rtt_ms(&self,) -> Option<u32,> {
        self.last_ping_rtt.map(|d| d.as_millis() as u32,)
    }
}
