// Intent citation: .kiro/specs/model-download-engine/design.md — Priority Queue
// Priority-ordered download queue using BinaryHeap.

use super::events::DownloadId;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use uuid::Uuid;

/// A download entry in the priority queue.
/// Lower priority number = higher priority (0 is highest).
#[derive(Debug, Clone)]
pub struct QueuedDownload {
    /// Unique download identifier.
    pub id: DownloadId,
    /// Priority level (0 = highest, 255 = lowest).
    pub priority: u8,
    /// Model/resource identifier.
    pub model_id: String,
    /// Submission timestamp for FIFO ordering within same priority.
    pub submitted_at_ms: u64,
}

/// Custom ordering: lower priority number = higher priority in the heap.
/// For equal priorities, earlier submission time wins (FIFO).
impl Ord for QueuedDownload {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse priority comparison (lower number = higher priority = should come first)
        other
            .priority
            .cmp(&self.priority)
            .then_with(|| other.submitted_at_ms.cmp(&self.submitted_at_ms))
    }
}

impl PartialOrd for QueuedDownload {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for QueuedDownload {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for QueuedDownload {}

/// Priority queue for pending downloads.
/// Always dequeues the highest-priority (lowest number) item first.
pub struct PriorityQueue {
    heap: BinaryHeap<QueuedDownload>,
}

impl PriorityQueue {
    /// Create an empty priority queue.
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }

    /// Insert a download into the queue with priority ordering.
    pub fn push(&mut self, download: QueuedDownload) {
        self.heap.push(download);
    }

    /// Remove and return the highest-priority item (lowest priority number).
    pub fn pop(&mut self) -> Option<QueuedDownload> {
        self.heap.pop()
    }

    /// View the highest-priority item without removing it.
    pub fn peek(&self) -> Option<&QueuedDownload> {
        self.heap.peek()
    }

    /// Remove a specific download by ID. Returns true if found and removed.
    pub fn remove(&mut self, id: &DownloadId) -> bool {
        let initial_len = self.heap.len();
        let items: Vec<_> = self.heap.drain().filter(|d| d.id != *id).collect();
        let removed = items.len() < initial_len;
        self.heap = BinaryHeap::from(items);
        removed
    }

    /// Number of items in the queue.
    pub fn len(&self) -> usize {
        self.heap.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.heap.is_empty()
    }
}

impl Default for PriorityQueue {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_queued(priority: u8, submitted_at_ms: u64) -> QueuedDownload {
        QueuedDownload {
            id: Uuid::new_v4(),
            priority,
            model_id: format!("model-p{}", priority),
            submitted_at_ms,
        }
    }

    #[test]
    fn test_empty_queue() {
        let mut queue = PriorityQueue::new();
        assert!(queue.is_empty());
        assert_eq!(queue.len(), 0);
        assert!(queue.pop().is_none());
        assert!(queue.peek().is_none());
    }

    #[test]
    fn test_priority_ordering() {
        let mut queue = PriorityQueue::new();

        queue.push(make_queued(100, 1000)); // Low priority
        queue.push(make_queued(1, 2000)); // High priority
        queue.push(make_queued(50, 3000)); // Medium priority

        // Should dequeue in priority order: 1, 50, 100
        assert_eq!(queue.pop().unwrap().priority, 1);
        assert_eq!(queue.pop().unwrap().priority, 50);
        assert_eq!(queue.pop().unwrap().priority, 100);
        assert!(queue.pop().is_none());
    }

    #[test]
    fn test_fifo_within_same_priority() {
        let mut queue = PriorityQueue::new();

        let first = make_queued(5, 1000);
        let second = make_queued(5, 2000);
        let third = make_queued(5, 3000);

        let first_id = first.id;
        let second_id = second.id;
        let third_id = third.id;

        queue.push(first);
        queue.push(second);
        queue.push(third);

        // Same priority — should be FIFO (earlier submitted_at_ms first)
        // Note: our ordering uses `other.submitted_at_ms.cmp(&self.submitted_at_ms)`
        // which means higher submitted_at_ms comes first in BinaryHeap (max-heap),
        // but we want lower submitted_at_ms first. Let me verify the logic:
        // For FIFO: earlier submission should come out first.
        // BinaryHeap is a max-heap, so the "greatest" element comes first.
        // Our Ord: other.submitted_at_ms.cmp(&self.submitted_at_ms)
        // This means: if other has higher submitted_at_ms, it's "greater" → comes first
        // Wait, that's LIFO. Let me fix the test to match actual behavior.
        // Actually: `other.submitted_at_ms.cmp(&self.submitted_at_ms)` means
        // self is "greater" when self.submitted_at_ms is LOWER (earlier).
        // So earlier submissions are "greater" in the heap → come out first. That's FIFO. ✓
        assert_eq!(queue.pop().unwrap().id, first_id);
        assert_eq!(queue.pop().unwrap().id, second_id);
        assert_eq!(queue.pop().unwrap().id, third_id);
    }

    #[test]
    fn test_peek_does_not_remove() {
        let mut queue = PriorityQueue::new();
        queue.push(make_queued(1, 1000));

        assert!(queue.peek().is_some());
        assert_eq!(queue.len(), 1);
        assert!(queue.peek().is_some());
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn test_remove_by_id() {
        let mut queue = PriorityQueue::new();

        let item = make_queued(5, 1000);
        let id = item.id;
        queue.push(item);
        queue.push(make_queued(10, 2000));

        assert_eq!(queue.len(), 2);
        assert!(queue.remove(&id));
        assert_eq!(queue.len(), 1);

        // Removing non-existent ID returns false
        assert!(!queue.remove(&Uuid::new_v4()));
    }

    #[test]
    fn test_len_and_is_empty() {
        let mut queue = PriorityQueue::new();
        assert!(queue.is_empty());

        queue.push(make_queued(1, 1000));
        assert!(!queue.is_empty());
        assert_eq!(queue.len(), 1);

        queue.push(make_queued(2, 2000));
        assert_eq!(queue.len(), 2);

        queue.pop();
        assert_eq!(queue.len(), 1);

        queue.pop();
        assert!(queue.is_empty());
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// **Validates: Requirements 7.1, 7.2**
        /// Property 2: Priority Ordering — items always dequeued in priority order
        /// (lowest number first). Within same priority, FIFO order is maintained.
        #[test]
        fn dequeue_always_in_priority_order(
            priorities in proptest::collection::vec(any::<u8>(), 1..50)
        ) {
            let mut queue = PriorityQueue::new();

            // Insert items with given priorities and sequential timestamps
            for (i, &priority) in priorities.iter().enumerate() {
                queue.push(QueuedDownload {
                    id: Uuid::from_u128(i as u128),
                    priority,
                    model_id: format!("model-{}", i),
                    submitted_at_ms: i as u64 * 1000,
                });
            }

            // Dequeue all and verify priority ordering
            let mut last_priority: Option<u8> = None;
            while let Some(item) = queue.pop() {
                if let Some(prev) = last_priority {
                    prop_assert!(
                        item.priority >= prev,
                        "Priority ordering violated: got {} after {}",
                        item.priority,
                        prev
                    );
                }
                last_priority = Some(item.priority);
            }

            prop_assert!(queue.is_empty());
        }
    }
}
