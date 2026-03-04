// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Memory pool for reusable transform buffers.
//!
//! Avoids repeated allocation/deallocation in hot loops by recycling
//! `Vec<u8>` buffers of appropriate capacity.

use std::sync::Mutex;

/// Thread-safe pool of reusable byte buffers.
pub struct BufferPool {
    pool: Mutex<Vec<Vec<u8>>>,
    max_cached: usize,
}

impl BufferPool {
    /// Create a pool caching up to `max_cached` buffers.
    #[must_use]
    pub fn new(max_cached: usize) -> Self {
        Self {
            pool: Mutex::new(Vec::with_capacity(max_cached)),
            max_cached,
        }
    }

    /// Acquire a buffer with at least `capacity` bytes.
    ///
    /// Returns a recycled buffer (cleared but with capacity retained)
    /// or allocates a new one.
    pub fn acquire(&self, capacity: usize) -> Vec<u8> {
        let mut pool = self.pool.lock().unwrap();
        // Find a buffer with sufficient capacity.
        if let Some(pos) = pool.iter().position(|b| b.capacity() >= capacity) {
            let mut buf = pool.swap_remove(pos);
            buf.clear();
            buf
        } else {
            Vec::with_capacity(capacity)
        }
    }

    /// Return a buffer to the pool for reuse.
    ///
    /// Buffers exceeding 1 MB are dropped rather than cached to avoid
    /// holding large allocations indefinitely.
    pub fn release(&self, buf: Vec<u8>) {
        if buf.capacity() > 1024 * 1024 {
            return; // Don't cache oversized buffers
        }
        let mut pool = self.pool.lock().unwrap();
        if pool.len() < self.max_cached {
            pool.push(buf);
        }
        // else: just drop it
    }

    /// Number of buffers currently cached.
    pub fn cached_count(&self) -> usize {
        self.pool.lock().unwrap().len()
    }
}

impl Default for BufferPool {
    fn default() -> Self {
        Self::new(16)
    }
}

/// Global buffer pool for transform operations.
pub fn global_pool() -> &'static BufferPool {
    use std::sync::OnceLock;
    static POOL: OnceLock<BufferPool> = OnceLock::new();
    POOL.get_or_init(|| BufferPool::new(32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_release_reuse() {
        let pool = BufferPool::new(4);
        let buf = pool.acquire(1024);
        assert!(buf.capacity() >= 1024);
        pool.release(buf);
        assert_eq!(pool.cached_count(), 1);

        // Should get the cached buffer back
        let buf2 = pool.acquire(512);
        assert!(buf2.capacity() >= 1024); // original capacity retained
        assert_eq!(pool.cached_count(), 0);
    }

    #[test]
    fn oversized_not_cached() {
        let pool = BufferPool::new(4);
        let buf = Vec::with_capacity(2 * 1024 * 1024);
        pool.release(buf);
        assert_eq!(pool.cached_count(), 0);
    }

    #[test]
    fn max_cached_respected() {
        let pool = BufferPool::new(2);
        pool.release(Vec::with_capacity(100));
        pool.release(Vec::with_capacity(100));
        pool.release(Vec::with_capacity(100)); // should be dropped
        assert_eq!(pool.cached_count(), 2);
    }

    #[test]
    fn global_pool_works() {
        let p = global_pool();
        let buf = p.acquire(64);
        assert!(buf.capacity() >= 64);
        p.release(buf);
    }
}
