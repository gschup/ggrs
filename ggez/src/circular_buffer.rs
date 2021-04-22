use std::collections::VecDeque;

/// A CircularBuffer that drops the oldest element inserted when full.
/// # Example
///
/// ```rust
/// use ggez::circular_buffer::CircularBuffer;
/// use std::collections::VecDeque;
///
/// let mut buf = CircularBuffer::<u32>::new(2);
/// assert_eq!(*buf.queue(), VecDeque::<u32>::from(vec![]));
/// assert!(buf.push_back(1).is_none());
/// assert_eq!(*buf.queue(), VecDeque::<u32>::from(vec![1]));
/// assert!(buf.push_back(2).is_none());
/// assert_eq!(*buf.queue(), VecDeque::<u32>::from(vec![1, 2]));
/// assert!(buf.push_back(3).is_some());
/// assert_eq!(*buf.queue(), VecDeque::<u32>::from(vec![2, 3]));
/// assert_eq!(buf.capacity(), 2);
/// ```
#[derive(Debug, Default)]
pub struct CircularBuffer<A> {
    queue: VecDeque<A>,
    cap: usize,
}

impl<A> CircularBuffer<A> {
    /// Creates a new CircularBuffer with fixed size `cap`.
    pub fn new(size: usize) -> Self {
        CircularBuffer {
            queue: VecDeque::with_capacity(size),
            cap: size,
        }
    }

    /// Add a value to the CircularBuffer. Returns the popped value if the buffer is full.
    pub fn push_back(&mut self, elem: A) -> Option<A> {
        let out = if self.queue.len() == self.cap {
            //back to front <->  newest to oldest
            self.queue.pop_front()
        } else {
            None
        };

        self.queue.push_back(elem);
        out
    }

    pub fn front(&self) -> Option<&A> {
        self.queue.front()
    }

    pub fn back(&self) -> Option<&A> {
        self.queue.back()
    }

    pub fn pop_front(&mut self) -> Option<A> {
        self.queue.pop_front()
    }

    pub fn pop_back(&mut self) -> Option<A> {
        self.queue.pop_back()
    }

    /// Get an immutable reference to the values inside the CircularBuffer.
    pub fn queue(&self) -> &VecDeque<A> {
        &self.queue
    }

    /// Returns the capacity of the circular buffer.
    pub fn capacity(&self) -> usize {
        self.cap
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}