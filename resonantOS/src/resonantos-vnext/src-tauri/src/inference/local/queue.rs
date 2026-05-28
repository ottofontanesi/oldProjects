// Request queue for local inference — FIFO per model.

use std::collections::{HashMap, VecDeque};

/// A queued inference request.
#[derive(Debug, Clone)]
pub struct QueuedRequest {
    pub request_id: String,
    pub model_id: String,
    pub prompt: String,
    pub queued_at_ms: u64,
}

/// Request queue manager — FIFO per model with concurrency limits.
pub struct RequestQueue {
    queues: HashMap<String, VecDeque<QueuedRequest>>,
    active_counts: HashMap<String, usize>,
    max_concurrent: usize,
}

impl RequestQueue {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            queues: HashMap::new(),
            active_counts: HashMap::new(),
            max_concurrent,
        }
    }

    /// Enqueue a request. Returns position in queue (0 = will run immediately).
    pub fn enqueue(&mut self, request: QueuedRequest) -> usize {
        let queue = self.queues.entry(request.model_id.clone()).or_default();
        queue.push_back(request);
        queue.len() - 1
    }

    /// Try to dequeue the next request for a model (if under concurrency limit).
    pub fn try_dequeue(&mut self, model_id: &str) -> Option<QueuedRequest> {
        let active = self.active_counts.get(model_id).copied().unwrap_or(0);
        if active >= self.max_concurrent {
            return None;
        }

        let queue = self.queues.get_mut(model_id)?;
        let request = queue.pop_front()?;
        *self.active_counts.entry(model_id.to_string()).or_insert(0) += 1;
        Some(request)
    }

    /// Mark a request as completed (decrements active count).
    pub fn complete(&mut self, model_id: &str) {
        if let Some(count) = self.active_counts.get_mut(model_id) {
            *count = count.saturating_sub(1);
        }
    }

    /// Get queue depth for a model.
    pub fn queue_depth(&self, model_id: &str) -> usize {
        self.queues.get(model_id).map(|q| q.len()).unwrap_or(0)
    }

    /// Get active request count for a model.
    pub fn active_count(&self, model_id: &str) -> usize {
        self.active_counts.get(model_id).copied().unwrap_or(0)
    }

    /// Total pending requests across all models.
    pub fn total_pending(&self) -> usize {
        self.queues.values().map(|q| q.len()).sum()
    }

    /// Total active requests across all models.
    pub fn total_active(&self) -> usize {
        self.active_counts.values().sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(model: &str, id: &str) -> QueuedRequest {
        QueuedRequest {
            request_id: id.to_string(),
            model_id: model.to_string(),
            prompt: "test".to_string(),
            queued_at_ms: 0,
        }
    }

    #[test]
    fn test_enqueue_and_dequeue() {
        let mut queue = RequestQueue::new(4);
        queue.enqueue(make_request("llama", "r1"));
        queue.enqueue(make_request("llama", "r2"));

        assert_eq!(queue.queue_depth("llama"), 2);

        let req = queue.try_dequeue("llama").unwrap();
        assert_eq!(req.request_id, "r1");
        assert_eq!(queue.active_count("llama"), 1);
    }

    #[test]
    fn test_concurrency_limit() {
        let mut queue = RequestQueue::new(2);
        queue.enqueue(make_request("llama", "r1"));
        queue.enqueue(make_request("llama", "r2"));
        queue.enqueue(make_request("llama", "r3"));

        queue.try_dequeue("llama").unwrap(); // active=1
        queue.try_dequeue("llama").unwrap(); // active=2
        let result = queue.try_dequeue("llama"); // blocked
        assert!(result.is_none());
    }

    #[test]
    fn test_complete_frees_slot() {
        let mut queue = RequestQueue::new(1);
        queue.enqueue(make_request("llama", "r1"));
        queue.enqueue(make_request("llama", "r2"));

        queue.try_dequeue("llama").unwrap(); // active=1
        assert!(queue.try_dequeue("llama").is_none()); // blocked

        queue.complete("llama"); // active=0
        let req = queue.try_dequeue("llama").unwrap(); // now works
        assert_eq!(req.request_id, "r2");
    }

    #[test]
    fn test_fifo_order() {
        let mut queue = RequestQueue::new(10);
        queue.enqueue(make_request("llama", "first"));
        queue.enqueue(make_request("llama", "second"));
        queue.enqueue(make_request("llama", "third"));

        assert_eq!(queue.try_dequeue("llama").unwrap().request_id, "first");
        assert_eq!(queue.try_dequeue("llama").unwrap().request_id, "second");
        assert_eq!(queue.try_dequeue("llama").unwrap().request_id, "third");
    }

    #[test]
    fn test_independent_model_queues() {
        let mut queue = RequestQueue::new(1);
        queue.enqueue(make_request("llama", "r1"));
        queue.enqueue(make_request("qwen", "r2"));

        // Both can dequeue (different models)
        assert!(queue.try_dequeue("llama").is_some());
        assert!(queue.try_dequeue("qwen").is_some());
    }
}
