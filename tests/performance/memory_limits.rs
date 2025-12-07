//! Memory limit detection and allocation guards for performance tests
//!
//! This module provides utilities to detect available system memory and enforce
//! safe allocation limits to prevent out-of-memory conditions in test environments,
//! particularly in containerized environments with cgroup limits.

use std::fs;

/// Estimated memory usage per MessageMetadata struct in bytes
/// This includes:
/// - Struct overhead: ~120 bytes
/// - String heap allocations (id, thread_id, sender_email, sender_domain, sender_name, subject): ~250 bytes
/// - Vec<String> for recipients and labels: ~100 bytes
/// - Total: ~470 bytes, rounded up to 512 for safety margin
pub const ESTIMATED_BYTES_PER_MESSAGE: usize = 512;

/// Default maximum memory percentage to use for test allocations (50%)
/// This leaves headroom for the test framework, classifier, and other operations
pub const DEFAULT_MAX_MEMORY_PERCENT: f64 = 0.50;

/// Absolute minimum memory required to run tests (64 MB)
pub const MIN_MEMORY_BYTES: usize = 64 * 1024 * 1024;

/// Default fallback if memory detection fails (256 MB)
pub const FALLBACK_AVAILABLE_MEMORY: usize = 256 * 1024 * 1024;

/// Hard cap on maximum messages to prevent runaway allocations (500,000)
pub const ABSOLUTE_MAX_MESSAGES: usize = 500_000;

/// Memory information from the system
#[derive(Debug, Clone)]
pub struct MemoryInfo {
    /// Total system memory in bytes
    pub total_bytes: usize,
    /// Available/free memory in bytes
    pub available_bytes: usize,
    /// Cgroup memory limit in bytes (if applicable)
    pub cgroup_limit_bytes: Option<usize>,
    /// Source of the memory information
    pub source: MemorySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemorySource {
    /// Read from /proc/meminfo
    ProcMeminfo,
    /// Read from cgroup v1 memory controller
    CgroupV1,
    /// Read from cgroup v2 memory controller
    CgroupV2,
    /// Fallback default value
    Fallback,
}

impl MemoryInfo {
    /// Get the effective memory limit considering cgroup constraints
    pub fn effective_limit(&self) -> usize {
        match self.cgroup_limit_bytes {
            Some(cgroup_limit) if cgroup_limit < self.total_bytes => cgroup_limit,
            _ => self.total_bytes,
        }
    }

    /// Get effective available memory considering cgroup constraints
    pub fn effective_available(&self) -> usize {
        let effective_limit = self.effective_limit();
        // If we have cgroup limits, available is the minimum of system available
        // and the cgroup limit minus current usage
        self.available_bytes.min(effective_limit)
    }
}

/// Detect system memory information
///
/// Attempts to read memory info from:
/// 1. cgroup v2 memory controller
/// 2. cgroup v1 memory controller
/// 3. /proc/meminfo
/// 4. Falls back to conservative defaults
pub fn detect_memory() -> MemoryInfo {
    // Try cgroup v2 first (modern containers)
    if let Some(info) = detect_cgroup_v2_memory() {
        return info;
    }

    // Try cgroup v1 (older containers)
    if let Some(info) = detect_cgroup_v1_memory() {
        return info;
    }

    // Fall back to /proc/meminfo (bare metal or VMs)
    if let Some(info) = detect_proc_meminfo() {
        return info;
    }

    // Ultimate fallback
    MemoryInfo {
        total_bytes: FALLBACK_AVAILABLE_MEMORY,
        available_bytes: FALLBACK_AVAILABLE_MEMORY,
        cgroup_limit_bytes: None,
        source: MemorySource::Fallback,
    }
}

/// Detect memory from cgroup v2 (unified hierarchy)
fn detect_cgroup_v2_memory() -> Option<MemoryInfo> {
    // Check if cgroup v2 is mounted
    let memory_max = fs::read_to_string("/sys/fs/cgroup/memory.max").ok()?;
    let memory_current = fs::read_to_string("/sys/fs/cgroup/memory.current").ok()?;

    let limit_bytes = if memory_max.trim() == "max" {
        // No limit set, use system memory
        None
    } else {
        memory_max.trim().parse::<usize>().ok()
    };

    let current_bytes = memory_current.trim().parse::<usize>().ok()?;

    // Also get total system memory for comparison
    let (total, _) =
        parse_proc_meminfo().unwrap_or((FALLBACK_AVAILABLE_MEMORY, FALLBACK_AVAILABLE_MEMORY));

    let effective_limit = limit_bytes.unwrap_or(total);
    let available = effective_limit.saturating_sub(current_bytes);

    Some(MemoryInfo {
        total_bytes: total,
        available_bytes: available,
        cgroup_limit_bytes: limit_bytes,
        source: MemorySource::CgroupV2,
    })
}

/// Detect memory from cgroup v1 memory controller
fn detect_cgroup_v1_memory() -> Option<MemoryInfo> {
    // Try to read cgroup v1 memory limit
    let limit_path = "/sys/fs/cgroup/memory/memory.limit_in_bytes";
    let usage_path = "/sys/fs/cgroup/memory/memory.usage_in_bytes";

    let limit_str = fs::read_to_string(limit_path).ok()?;
    let usage_str = fs::read_to_string(usage_path).ok()?;

    let limit_bytes = limit_str.trim().parse::<usize>().ok()?;
    let usage_bytes = usage_str.trim().parse::<usize>().ok()?;

    // Get total system memory
    let (total, _) =
        parse_proc_meminfo().unwrap_or((FALLBACK_AVAILABLE_MEMORY, FALLBACK_AVAILABLE_MEMORY));

    // cgroup v1 reports a very high number (like 9223372036854771712) when unlimited
    // Check if limit is unreasonably high (more than 1 TB)
    let effective_limit = if limit_bytes > 1024 * 1024 * 1024 * 1024 {
        None
    } else {
        Some(limit_bytes)
    };

    let available = effective_limit.unwrap_or(total).saturating_sub(usage_bytes);

    Some(MemoryInfo {
        total_bytes: total,
        available_bytes: available,
        cgroup_limit_bytes: effective_limit,
        source: MemorySource::CgroupV1,
    })
}

/// Detect memory from /proc/meminfo
fn detect_proc_meminfo() -> Option<MemoryInfo> {
    let (total, available) = parse_proc_meminfo()?;

    Some(MemoryInfo {
        total_bytes: total,
        available_bytes: available,
        cgroup_limit_bytes: None,
        source: MemorySource::ProcMeminfo,
    })
}

/// Parse /proc/meminfo and return (total, available) in bytes
fn parse_proc_meminfo() -> Option<(usize, usize)> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;

    let mut total_kb = 0usize;
    let mut available_kb = 0usize;
    let mut free_kb = 0usize;
    let mut buffers_kb = 0usize;
    let mut cached_kb = 0usize;

    for line in content.lines() {
        if let Some(value) = parse_meminfo_line(line, "MemTotal:") {
            total_kb = value;
        } else if let Some(value) = parse_meminfo_line(line, "MemAvailable:") {
            available_kb = value;
        } else if let Some(value) = parse_meminfo_line(line, "MemFree:") {
            free_kb = value;
        } else if let Some(value) = parse_meminfo_line(line, "Buffers:") {
            buffers_kb = value;
        } else if let Some(value) = parse_meminfo_line(line, "Cached:") {
            cached_kb = value;
        }
    }

    if total_kb == 0 {
        return None;
    }

    // MemAvailable is the best indicator of available memory
    // If not present (older kernels), estimate from free + buffers + cached
    let available = if available_kb > 0 {
        available_kb
    } else {
        free_kb + buffers_kb + cached_kb
    };

    Some((total_kb * 1024, available * 1024))
}

/// Parse a line from /proc/meminfo
fn parse_meminfo_line(line: &str, prefix: &str) -> Option<usize> {
    if line.starts_with(prefix) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 {
            return parts[1].parse().ok();
        }
    }
    None
}

/// Calculate the maximum safe message count based on available memory
///
/// # Arguments
/// * `memory_info` - Memory information from detect_memory()
/// * `max_memory_percent` - Maximum percentage of available memory to use (0.0-1.0)
///
/// # Returns
/// Maximum number of messages that can be safely allocated
pub fn calculate_max_messages(memory_info: &MemoryInfo, max_memory_percent: f64) -> usize {
    let available = memory_info.effective_available();

    // Ensure we have minimum required memory
    if available < MIN_MEMORY_BYTES {
        eprintln!(
            "Warning: Very low available memory ({} MB). Tests may be unstable.",
            available / (1024 * 1024)
        );
        return 1000; // Return minimal safe value
    }

    // Calculate usable memory
    let usable_bytes = (available as f64 * max_memory_percent) as usize;

    // Calculate max messages
    let max_messages = usable_bytes / ESTIMATED_BYTES_PER_MESSAGE;

    // Apply absolute cap
    max_messages.min(ABSOLUTE_MAX_MESSAGES)
}

/// Calculate max messages using default settings
pub fn calculate_max_messages_default() -> usize {
    let memory_info = detect_memory();
    calculate_max_messages(&memory_info, DEFAULT_MAX_MEMORY_PERCENT)
}

/// Check if a requested count exceeds safe limits and return adjusted count
///
/// # Arguments
/// * `requested_count` - The number of messages requested
///
/// # Returns
/// A tuple of (adjusted_count, was_limited) where was_limited is true if the
/// count was reduced
pub fn apply_memory_limit(requested_count: usize) -> (usize, bool) {
    let max_safe = calculate_max_messages_default();

    if requested_count > max_safe {
        eprintln!(
            "Warning: Requested {} messages exceeds safe limit of {} based on available memory. \
             Reducing to safe limit.",
            requested_count, max_safe
        );
        (max_safe, true)
    } else {
        (requested_count, false)
    }
}

/// Error returned when allocation would exceed memory limits
#[derive(Debug, Clone)]
pub struct MemoryLimitExceeded {
    pub requested_count: usize,
    pub max_safe_count: usize,
    pub available_memory_bytes: usize,
}

impl std::fmt::Display for MemoryLimitExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Memory limit exceeded: requested {} messages but only {} safe \
             (available memory: {} MB)",
            self.requested_count,
            self.max_safe_count,
            self.available_memory_bytes / (1024 * 1024)
        )
    }
}

impl std::error::Error for MemoryLimitExceeded {}

/// Check if allocation is safe, returning an error if not
///
/// Unlike `apply_memory_limit`, this function does not adjust the count
/// but instead returns an error if the limit would be exceeded.
pub fn check_allocation_safe(requested_count: usize) -> Result<(), MemoryLimitExceeded> {
    let memory_info = detect_memory();
    let max_safe = calculate_max_messages(&memory_info, DEFAULT_MAX_MEMORY_PERCENT);

    if requested_count > max_safe {
        Err(MemoryLimitExceeded {
            requested_count,
            max_safe_count: max_safe,
            available_memory_bytes: memory_info.effective_available(),
        })
    } else {
        Ok(())
    }
}

/// Print memory diagnostic information
pub fn print_memory_diagnostics() {
    let info = detect_memory();
    let max_messages = calculate_max_messages(&info, DEFAULT_MAX_MEMORY_PERCENT);

    println!("=== Memory Diagnostics ===");
    println!("Source: {:?}", info.source);
    println!(
        "Total system memory: {} MB",
        info.total_bytes / (1024 * 1024)
    );
    println!(
        "Available memory: {} MB",
        info.available_bytes / (1024 * 1024)
    );
    if let Some(cgroup_limit) = info.cgroup_limit_bytes {
        println!("Cgroup limit: {} MB", cgroup_limit / (1024 * 1024));
    }
    println!(
        "Effective available: {} MB",
        info.effective_available() / (1024 * 1024)
    );
    println!(
        "Estimated bytes per message: {}",
        ESTIMATED_BYTES_PER_MESSAGE
    );
    println!("Max safe message count: {}", max_messages);
    println!("========================");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_memory_returns_valid_info() {
        let info = detect_memory();

        // Should always return some value
        assert!(info.total_bytes > 0, "Total memory should be > 0");
        assert!(info.available_bytes > 0, "Available memory should be > 0");
        assert!(
            info.available_bytes <= info.total_bytes,
            "Available should be <= total"
        );
    }

    #[test]
    fn test_calculate_max_messages() {
        let info = MemoryInfo {
            total_bytes: 1024 * 1024 * 1024,    // 1 GB
            available_bytes: 512 * 1024 * 1024, // 512 MB
            cgroup_limit_bytes: None,
            source: MemorySource::Fallback,
        };

        let max = calculate_max_messages(&info, 0.5);

        // 512 MB * 0.5 = 256 MB
        // 256 MB / 512 bytes per message = 524,288 messages
        // But capped at ABSOLUTE_MAX_MESSAGES = 500,000
        assert!(max > 0, "Max messages should be > 0");
        assert!(
            max <= ABSOLUTE_MAX_MESSAGES,
            "Should be capped at absolute max"
        );
    }

    #[test]
    fn test_calculate_max_messages_with_cgroup_limit() {
        let info = MemoryInfo {
            total_bytes: 8 * 1024 * 1024 * 1024,         // 8 GB system
            available_bytes: 4 * 1024 * 1024 * 1024,     // 4 GB available
            cgroup_limit_bytes: Some(256 * 1024 * 1024), // 256 MB cgroup limit
            source: MemorySource::CgroupV1,
        };

        let max = calculate_max_messages(&info, 0.5);

        // Effective available should be min(4GB, 256MB) = 256MB
        // 256 MB * 0.5 = 128 MB
        // 128 MB / 512 bytes = 262,144 messages
        assert!(max > 0);
        assert!(max <= 262_144, "Should respect cgroup limit: got {}", max);
    }

    #[test]
    fn test_apply_memory_limit_no_reduction() {
        let (count, was_limited) = apply_memory_limit(100);
        assert_eq!(count, 100, "Small count should not be reduced");
        assert!(!was_limited, "Should not be limited");
    }

    #[test]
    fn test_check_allocation_safe_small() {
        let result = check_allocation_safe(100);
        assert!(result.is_ok(), "Small allocation should be safe");
    }

    #[test]
    fn test_memory_info_effective_limit() {
        let info_no_cgroup = MemoryInfo {
            total_bytes: 1024 * 1024 * 1024,
            available_bytes: 512 * 1024 * 1024,
            cgroup_limit_bytes: None,
            source: MemorySource::ProcMeminfo,
        };
        assert_eq!(info_no_cgroup.effective_limit(), 1024 * 1024 * 1024);

        let info_with_cgroup = MemoryInfo {
            total_bytes: 8 * 1024 * 1024 * 1024,
            available_bytes: 4 * 1024 * 1024 * 1024,
            cgroup_limit_bytes: Some(256 * 1024 * 1024),
            source: MemorySource::CgroupV1,
        };
        assert_eq!(info_with_cgroup.effective_limit(), 256 * 1024 * 1024);
    }

    #[test]
    fn test_estimated_bytes_per_message_is_reasonable() {
        // Each MessageMetadata has:
        // - Multiple Strings (each String is 24 bytes on stack + heap data)
        // - Vec<String> for recipients and labels
        // - DateTime (12 bytes)
        // - 2 bools (2 bytes)
        // 512 bytes should be a safe upper bound
        assert!(
            ESTIMATED_BYTES_PER_MESSAGE >= 256,
            "Estimate should account for heap allocations"
        );
        assert!(
            ESTIMATED_BYTES_PER_MESSAGE <= 1024,
            "Estimate should not be unreasonably high"
        );
    }

    #[test]
    fn test_print_memory_diagnostics_does_not_panic() {
        // Just ensure it doesn't panic
        print_memory_diagnostics();
    }
}
