//! Load balancing strategies for upstream server selection.
//!
//! This module will contain implementations of various load balancing algorithms
//! such as round-robin, least connections, weighted distribution, etc.

use crate::upstream::Upstream;
use std::sync::atomic::{AtomicUsize, Ordering};

/// A round-robin load balancer that distributes requests evenly across upstream servers.
///
/// This load balancer maintains an internal counter that increments for each request,
/// and uses modulo arithmetic to cycle through the available servers in order.
/// The set of servers is immutable once created.
#[derive(Debug)]
pub struct LoadBalancer {
    /// The list of upstream servers to balance requests across
    servers: Vec<Upstream>,
    /// The current index for round-robin selection (atomic for thread safety)
    current_index: AtomicUsize,
}

impl LoadBalancer {
    /// Creates a new load balancer with the given upstream servers.
    ///
    /// # Arguments
    ///
    /// * `servers` - A vector of upstream servers to balance requests across
    ///
    /// # Returns
    ///
    /// A new `LoadBalancer` instance
    pub fn new(servers: Vec<Upstream>) -> Self {
        assert!(
            !servers.is_empty(),
            "Amount of servers should be greater than 0"
        );
        Self {
            servers,
            current_index: AtomicUsize::new(0),
        }
    }

    /// Selects the next upstream server using round-robin algorithm.
    ///
    /// # Returns
    ///
    /// - `Some(Upstream)` if servers are available
    /// - `None` if no servers are configured
    pub fn get_upstream(&self) -> *const Upstream {
        let current = self.current_index.fetch_add(1, Ordering::Relaxed);
        let index = current % self.servers.len();

        (unsafe { self.servers.get_unchecked(index) }) as *const _
    }
}
