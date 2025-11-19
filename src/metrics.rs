/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 *
 *  Copyright (C) 2023-2025  Mles developers
 *
 * metrics.rs - centralized metrics for the mles server
 *
 * This module provides a simple, thread-safe metrics container used across
 * the server (websocket, server, http, etc.). The API is intentionally small
 * and uses standard library synchronization primitives to avoid adding extra
 * dependencies.
 */

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Centralized metrics structure.
///
/// Clone is cheap because it clones Arcs to the underlying counters/maps.
#[derive(Clone)]
pub struct Metrics {
    pub active_connections: Arc<AtomicUsize>,
    pub total_messages_sent: Arc<AtomicU64>,
    pub total_messages_received: Arc<AtomicU64>,
    pub total_channels: Arc<AtomicUsize>,
    pub total_errors: Arc<AtomicU64>,

    // Dropped connections due to per-IP limiting
    pub total_dropped_connections: Arc<AtomicU64>,
    // Per-IP dropped counts (stringified ip -> count)
    pub dropped_by_ip: Arc<Mutex<HashMap<String, u64>>>,
}

impl Metrics {
    /// Create a new, empty Metrics container.
    pub fn new() -> Self {
        Self {
            active_connections: Arc::new(AtomicUsize::new(0)),
            total_messages_sent: Arc::new(AtomicU64::new(0)),
            total_messages_received: Arc::new(AtomicU64::new(0)),
            total_channels: Arc::new(AtomicUsize::new(0)),
            total_errors: Arc::new(AtomicU64::new(0)),
            total_dropped_connections: Arc::new(AtomicU64::new(0)),
            dropped_by_ip: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Snapshot current metrics into a small, owned struct suitable for logging/inspection.
    pub fn get_stats(&self) -> MetricsSnapshot {
        // Snapshot the dropped_by_ip map
        let dropped_map = {
            // Prefer to panic only in the very unlikely poisoned-lock case;
            // we could handle it differently, but for now unwrap is acceptable.
            let guard = self.dropped_by_ip.lock().unwrap();
            guard.clone()
        };

        MetricsSnapshot {
            active_connections: self.active_connections.load(Ordering::Relaxed),
            total_messages_sent: self.total_messages_sent.load(Ordering::Relaxed),
            total_messages_received: self.total_messages_received.load(Ordering::Relaxed),
            total_channels: self.total_channels.load(Ordering::Relaxed),
            total_errors: self.total_errors.load(Ordering::Relaxed),
            total_dropped_connections: self.total_dropped_connections.load(Ordering::Relaxed),
            dropped_by_ip: dropped_map,
        }
    }

    // ---- Convenience increment/decrement helpers (keep these fast) ----

    pub fn increment_connections(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrement_connections(&self) {
        self.active_connections.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn increment_messages_sent(&self) {
        self.total_messages_sent.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_messages_received(&self) {
        self.total_messages_received.fetch_add(1, Ordering::Relaxed);
    }

    pub fn increment_errors(&self) {
        self.total_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn update_channel_count(&self, count: usize) {
        self.total_channels.store(count, Ordering::Relaxed);
    }

    /// Increment drop counters for the given source IP (string form).
    /// This increments both the per-IP counter and the total dropped connections counter.
    pub fn increment_dropped_by_ip(&self, ip: &str) {
        // Update per-IP map
        if let Ok(mut map) = self.dropped_by_ip.lock() {
            let counter = map.entry(ip.to_string()).or_insert(0);
            *counter = counter.saturating_add(1);
        } else {
            // In case of a poisoned lock, still make progress on the aggregate counter.
            // (Avoid unwrap/panic here to keep the hot path resilient.)
        }
        // Update aggregate
        self.total_dropped_connections
            .fetch_add(1, Ordering::Relaxed);
    }
}

/// Lightweight owned snapshot of metrics suitable for logging, tests or exporting.
#[derive(Debug, Clone)]
pub struct MetricsSnapshot {
    pub active_connections: usize,
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
    pub total_channels: usize,
    pub total_errors: u64,
    pub total_dropped_connections: u64,
    pub dropped_by_ip: HashMap<String, u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_basic_counts() {
        let m = Metrics::new();
        assert_eq!(m.get_stats().active_connections, 0);
        m.increment_connections();
        m.increment_connections();
        assert_eq!(m.get_stats().active_connections, 2);
        m.decrement_connections();
        assert_eq!(m.get_stats().active_connections, 1);

        m.increment_messages_sent();
        m.increment_messages_received();
        m.increment_errors();

        let s = m.get_stats();
        assert_eq!(s.total_messages_sent, 1);
        assert_eq!(s.total_messages_received, 1);
        assert_eq!(s.total_errors, 1);
    }

    #[test]
    fn metrics_dropped_by_ip() {
        let m = Metrics::new();
        m.increment_dropped_by_ip("1.2.3.4");
        m.increment_dropped_by_ip("1.2.3.4");
        m.increment_dropped_by_ip("10.0.0.1");

        let s = m.get_stats();
        assert_eq!(s.total_dropped_connections, 3);
        assert_eq!(s.dropped_by_ip.get("1.2.3.4"), Some(&2u64));
        assert_eq!(s.dropped_by_ip.get("10.0.0.1"), Some(&1u64));
    }
}
