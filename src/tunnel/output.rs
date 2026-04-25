//! Process output capture and per-entry log buffer.
//!
//! Provides [`LogLine`] and [`ProcessLog`], a bounded ring buffer that stores
//! timestamped output lines from tunnel processes. Each line carries a
//! [`LogStream`] tag indicating whether it came from stdout or stderr.

use std::collections::VecDeque;

use chrono::Local;

/// Maximum number of lines retained per entry.
const MAX_LOG_LINES: usize = 1000;

/// Which output stream a log line originated from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum LogStream {
    /// Process stdout.
    Stdout,
    /// Process stderr.
    Stderr,
    /// Internal system message (supervisor lifecycle, not from process).
    System,
}

/// A single line of captured process output.
#[derive(Debug, Clone)]
pub struct LogLine {
    /// Wall-clock timestamp when the line was received.
    pub timestamp: String,
    /// Which stream produced this line.
    pub stream: LogStream,
    /// The text content (no trailing newline).
    pub text: String,
}

/// Bounded ring buffer of [`LogLine`]s for a single tunnel entry.
#[derive(Debug, Clone)]
pub struct ProcessLog {
    lines: VecDeque<LogLine>,
}

impl Default for ProcessLog {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessLog {
    /// Create an empty log buffer.
    pub fn new() -> Self {
        Self {
            lines: VecDeque::with_capacity(MAX_LOG_LINES),
        }
    }

    /// Append a line, evicting the oldest if at capacity.
    pub fn push(&mut self, stream: LogStream, text: String) {
        if self.lines.len() >= MAX_LOG_LINES {
            self.lines.pop_front();
        }
        self.lines.push_back(LogLine {
            timestamp: Local::now().format("%H:%M:%S").to_string(),
            stream,
            text,
        });
    }

    /// Number of lines currently stored.
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Iterate over all stored lines in chronological order.
    pub fn iter(&self) -> impl Iterator<Item = &LogLine> {
        self.lines.iter()
    }

    /// Clear all stored lines.
    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_iterate() {
        let mut log = ProcessLog::new();
        log.push(LogStream::Stdout, "hello".to_string());
        log.push(LogStream::Stderr, "world".to_string());
        assert_eq!(log.len(), 2);
        let lines: Vec<&LogLine> = log.iter().collect();
        assert_eq!(lines[0].text, "hello");
        assert_eq!(lines[0].stream, LogStream::Stdout);
        assert_eq!(lines[1].text, "world");
        assert_eq!(lines[1].stream, LogStream::Stderr);
    }

    #[test]
    fn test_ring_buffer_eviction() {
        let mut log = ProcessLog::new();
        for i in 0..1100 {
            log.push(LogStream::Stdout, format!("line {i}"));
        }
        assert_eq!(log.len(), MAX_LOG_LINES);
        // Oldest retained should be line 100
        let first = log.iter().next().unwrap();
        assert_eq!(first.text, "line 100");
    }

    #[test]
    fn test_clear() {
        let mut log = ProcessLog::new();
        log.push(LogStream::System, "test".to_string());
        assert!(!log.is_empty());
        log.clear();
        assert!(log.is_empty());
    }
}
