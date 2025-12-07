//! Memory tracking allocator for debugging OOM issues in tests
//!
//! This allocator wraps the system allocator and tracks total memory usage.
//! When memory exceeds a configurable limit, it panics with a backtrace.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Maximum memory limit in bytes (8GB)
const MEMORY_LIMIT: usize = 8 * 1024 * 1024 * 1024;

/// Current allocated memory in bytes
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

/// Peak allocated memory in bytes
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Memory tracking allocator
pub struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let old = ALLOCATED.fetch_add(size, Ordering::SeqCst);
        let new = old + size;

        // Update peak
        let mut peak = PEAK.load(Ordering::SeqCst);
        while new > peak {
            match PEAK.compare_exchange_weak(peak, new, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(p) => peak = p,
            }
        }

        // Check limit
        if new > MEMORY_LIMIT {
            // Print diagnostic before panicking
            eprintln!("\n\n========================================");
            eprintln!("MEMORY LIMIT EXCEEDED!");
            eprintln!("========================================");
            eprintln!("Attempted allocation: {} bytes ({:.2} MB)", size, size as f64 / 1024.0 / 1024.0);
            eprintln!("Current total: {} bytes ({:.2} GB)", new, new as f64 / 1024.0 / 1024.0 / 1024.0);
            eprintln!("Limit: {} bytes ({:.2} GB)", MEMORY_LIMIT, MEMORY_LIMIT as f64 / 1024.0 / 1024.0 / 1024.0);
            eprintln!("Peak before this: {} bytes ({:.2} GB)", peak, peak as f64 / 1024.0 / 1024.0 / 1024.0);
            eprintln!("========================================\n");

            // Undo the allocation tracking since we're about to fail
            ALLOCATED.fetch_sub(size, Ordering::SeqCst);

            panic!(
                "Memory limit exceeded: tried to allocate {} bytes, total would be {:.2} GB (limit: {:.2} GB)",
                size,
                new as f64 / 1024.0 / 1024.0 / 1024.0,
                MEMORY_LIMIT as f64 / 1024.0 / 1024.0 / 1024.0
            );
        }

        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        ALLOCATED.fetch_sub(layout.size(), Ordering::SeqCst);
        System.dealloc(ptr, layout)
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let old_size = layout.size();

        if new_size > old_size {
            let diff = new_size - old_size;
            let old = ALLOCATED.fetch_add(diff, Ordering::SeqCst);
            let new_total = old + diff;

            // Update peak
            let mut peak = PEAK.load(Ordering::SeqCst);
            while new_total > peak {
                match PEAK.compare_exchange_weak(peak, new_total, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => break,
                    Err(p) => peak = p,
                }
            }

            if new_total > MEMORY_LIMIT {
                eprintln!("\n\n========================================");
                eprintln!("MEMORY LIMIT EXCEEDED IN REALLOC!");
                eprintln!("========================================");
                eprintln!("Realloc from {} to {} bytes", old_size, new_size);
                eprintln!("Current total: {} bytes ({:.2} GB)", new_total, new_total as f64 / 1024.0 / 1024.0 / 1024.0);
                eprintln!("Limit: {} bytes ({:.2} GB)", MEMORY_LIMIT, MEMORY_LIMIT as f64 / 1024.0 / 1024.0 / 1024.0);
                eprintln!("========================================\n");

                ALLOCATED.fetch_sub(diff, Ordering::SeqCst);

                panic!(
                    "Memory limit exceeded in realloc: total would be {:.2} GB (limit: {:.2} GB)",
                    new_total as f64 / 1024.0 / 1024.0 / 1024.0,
                    MEMORY_LIMIT as f64 / 1024.0 / 1024.0 / 1024.0
                );
            }
        } else {
            let diff = old_size - new_size;
            ALLOCATED.fetch_sub(diff, Ordering::SeqCst);
        }

        System.realloc(ptr, layout, new_size)
    }
}

/// Get current allocated memory in bytes
pub fn current_allocated() -> usize {
    ALLOCATED.load(Ordering::SeqCst)
}

/// Get peak allocated memory in bytes
pub fn peak_allocated() -> usize {
    PEAK.load(Ordering::SeqCst)
}

/// Print current memory stats
pub fn print_memory_stats(label: &str) {
    let current = current_allocated();
    let peak = peak_allocated();
    eprintln!(
        "[MEMORY] {}: current={:.2} MB, peak={:.2} MB",
        label,
        current as f64 / 1024.0 / 1024.0,
        peak as f64 / 1024.0 / 1024.0
    );
}
