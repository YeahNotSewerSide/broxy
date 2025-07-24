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
            servers.len() > 0,
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

    /// Returns the number of servers in the load balancer.
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn create_test_upstream(port: u16) -> Upstream {
        Upstream {
            address: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), port),
            root_path: "http://localhost/".parse().unwrap(),
            use_ssl: false,
        }
    }

    #[test]
    fn test_new_load_balancer() {
        let servers = vec![create_test_upstream(8001), create_test_upstream(8002)];

        let lb = LoadBalancer::new(servers);
        assert_eq!(lb.server_count(), 2);
    }

    #[test]
    fn test_empty_load_balancer() {
        let lb = LoadBalancer::new(Vec::new());
        assert_eq!(lb.server_count(), 0);
        assert!(lb.get_upstream().is_none());
    }

    #[test]
    fn test_round_robin_selection() {
        let servers = vec![
            create_test_upstream(8001),
            create_test_upstream(8002),
            create_test_upstream(8003),
        ];

        let lb = LoadBalancer::new(servers);

        // First selection should return server 0
        let server1 = lb.get_upstream().unwrap();
        assert_eq!(server1.address.port(), 8001);

        // Second selection should return server 1
        let server2 = lb.get_upstream().unwrap();
        assert_eq!(server2.address.port(), 8002);

        // Third selection should return server 2
        let server3 = lb.get_upstream().unwrap();
        assert_eq!(server3.address.port(), 8003);

        // Fourth selection should wrap around to server 0
        let server4 = lb.get_upstream().unwrap();
        assert_eq!(server4.address.port(), 8001);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let servers = vec![create_test_upstream(8001), create_test_upstream(8002)];

        let lb = Arc::new(LoadBalancer::new(servers));
        let mut handles = vec![];

        // Spawn multiple threads to test concurrent access
        for _ in 0..10 {
            let lb_clone = Arc::clone(&lb);
            let handle = thread::spawn(move || {
                for _ in 0..100 {
                    let server = lb_clone.get_upstream().unwrap();
                    assert!(server.address.port() == 8001 || server.address.port() == 8002);
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }
    }
}
