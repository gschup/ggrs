use crate::frame_info::PlayerInput;
use crate::{Config, Frame, InputStatus, NULL_FRAME};
use std::cmp;

/// The length of the input queue. This describes the number of inputs GGRS can hold at the same time per player.
const INPUT_QUEUE_LENGTH: usize = 128;

/// `InputQueue` handles inputs for a single player and saves them in a circular array. Valid Inputs are between `head` and `tail`.
#[derive(Debug, Clone)]
pub(crate) struct InputQueue<T>
where
    T: Config,
{
    /// The head of the queue. The newest `PlayerInput` is saved here
    head: usize,
    /// The tail of the queue. The oldest `PlayerInput` still valid is saved here.
    tail: usize,
    /// The current length of the queue.
    length: usize,
    /// Denotes if we still are in the first frame, an edge case to be considered by some methods.
    first_frame: bool,

    /// The last frame added by the user
    last_added_frame: Frame,
    /// The first frame in the queue that is known to be an incorrect prediction
    first_incorrect_frame: Frame,
    /// The last frame that has been requested. We make sure to never delete anything after this, as we would throw away important data.
    last_requested_frame: Frame,

    /// The delay in frames by which inputs are sent back to the user. This can be set during initialization.
    frame_delay: usize,

    /// Our cyclic input queue
    inputs: Vec<PlayerInput<T::Input>>,
    /// A pre-allocated prediction we are going to use to return predictions from.
    prediction: PlayerInput<T::Input>,
}

impl<T: Config> InputQueue<T> {
    pub(crate) fn new() -> Self {
        Self {
            head: 0,
            tail: 0,
            length: 0,
            frame_delay: 0,
            first_frame: true,
            last_added_frame: NULL_FRAME,
            first_incorrect_frame: NULL_FRAME,
            last_requested_frame: NULL_FRAME,
            prediction: PlayerInput::blank_input(NULL_FRAME),
            inputs: vec![PlayerInput::blank_input(NULL_FRAME); INPUT_QUEUE_LENGTH],
        }
    }

    pub(crate) fn first_incorrect_frame(&self) -> Frame {
        self.first_incorrect_frame
    }

    pub(crate) fn set_frame_delay(&mut self, delay: usize) {
        self.frame_delay = delay;
    }

    pub(crate) fn reset_prediction(&mut self) {
        self.prediction.frame = NULL_FRAME;
        self.first_incorrect_frame = NULL_FRAME;
        self.last_requested_frame = NULL_FRAME;
    }

    /// Returns a `PlayerInput`, but only if the input for the requested frame is confirmed.
    /// In contrast to `input()`, this will not return a prediction if there is no confirmed input for the frame, but panic instead.
    pub(crate) fn confirmed_input(&self, requested_frame: Frame) -> PlayerInput<T::Input> {
        let offset = requested_frame as usize % INPUT_QUEUE_LENGTH;

        if self.inputs[offset].frame == requested_frame {
            return self.inputs[offset];
        }

        // the requested confirmed input should not be before a prediction. We should not have asked for a known incorrect frame.
        panic!("SyncLayer::confirmed_input(): There is no confirmed input for the requested frame");
    }

    /// Discards confirmed frames up to given `frame` from the queue. All confirmed frames are guaranteed to be synchronized between players, so there is no need to save the inputs anymore.
    pub(crate) fn discard_confirmed_frames(&mut self, mut frame: Frame) {
        // we only drop frames until the last frame that was requested, otherwise we might delete data still needed
        if self.last_requested_frame != NULL_FRAME {
            frame = cmp::min(frame, self.last_requested_frame);
        }

        // move the tail to "delete inputs", wrap around if necessary
        if frame >= self.last_added_frame {
            // delete all but most recent
            self.tail = self.head;
            self.length = 1;
        } else if frame <= self.inputs[self.tail].frame {
            // we don't need to delete anything
        } else {
            let offset = (frame - (self.inputs[self.tail].frame)) as usize;
            self.tail = (self.tail + offset) % INPUT_QUEUE_LENGTH;
            self.length -= offset;
        }
    }

    /// Returns the game input of a single player for a given frame, if that input does not exist, we return a prediction instead.
    pub(crate) fn input(&mut self, requested_frame: Frame) -> (T::Input, InputStatus) {
        // No one should ever try to grab any input when we have a prediction error.
        // Doing so means that we're just going further down the wrong path. Assert this to verify that it's true.
        assert!(self.first_incorrect_frame == NULL_FRAME);

        // Remember the last requested frame number for later. We'll need this in add_input() to drop out of prediction mode.
        self.last_requested_frame = requested_frame;

        // assert that we request a frame that still exists
        assert!(requested_frame >= self.inputs[self.tail].frame);

        // We currently don't have a prediction frame
        if self.prediction.frame < 0 {
            //  If the frame requested is in our range, fetch it out of the queue and return it.
            let mut offset: usize = (requested_frame - self.inputs[self.tail].frame) as usize;

            if offset < self.length {
                offset = (offset + self.tail) % INPUT_QUEUE_LENGTH;
                assert!(self.inputs[offset].frame == requested_frame);
                return (self.inputs[offset].input, InputStatus::Confirmed);
            }

            // The requested frame isn't in the queue. This means we need to return a prediction frame. Predict that the user will do the same thing they did last time.
            if requested_frame == 0 || self.last_added_frame == NULL_FRAME {
                // basing new prediction frame from nothing, since we are on frame 0 or we have no frames yet
                self.prediction = PlayerInput::blank_input(self.prediction.frame);
            } else {
                // basing new prediction frame from previously added frame
                let previous_position = match self.head {
                    0 => INPUT_QUEUE_LENGTH - 1,
                    _ => self.head - 1,
                };
                self.prediction = self.inputs[previous_position];
            }
            // update the prediction's frame
            self.prediction.frame += 1;
        }

        // We must be predicting, so we return the prediction frame contents. We are adjusting the prediction to have the requested frame.
        assert!(self.prediction.frame != NULL_FRAME);
        let prediction_to_return = self.prediction; // PlayerInput has copy semantics
        (prediction_to_return.input, InputStatus::Predicted)
    }

    /// Adds an input frame to the queue. Will consider the set frame delay.
    pub(crate) fn add_input(&mut self, input: PlayerInput<T::Input>) -> Frame {
        // Verify that inputs are passed in sequentially by the user, regardless of frame delay.
        if self.last_added_frame != NULL_FRAME
            && input.frame + self.frame_delay as i32 != self.last_added_frame + 1
        {
            // drop the input if not given sequentially
            return NULL_FRAME;
        }

        // Move the queue head to the correct point in preparation to input the frame into the queue.
        let new_frame = self.advance_queue_head(input.frame);
        // if the frame is valid, then add the input
        if new_frame != NULL_FRAME {
            self.add_input_by_frame(input, new_frame);
        }
        new_frame
    }

    /// Adds an input frame to the queue at the given frame number. If there are predicted inputs, we will check those and mark them as incorrect, if necessary.
    /// Returns the frame number
    fn add_input_by_frame(&mut self, input: PlayerInput<T::Input>, frame_number: Frame) {
        let previous_position = match self.head {
            0 => INPUT_QUEUE_LENGTH - 1,
            _ => self.head - 1,
        };

        assert!(self.last_added_frame == NULL_FRAME || frame_number == self.last_added_frame + 1);
        assert!(frame_number == 0 || self.inputs[previous_position].frame == frame_number - 1);

        // Add the frame to the back of the queue
        self.inputs[self.head] = input;
        self.inputs[self.head].frame = frame_number;
        self.head = (self.head + 1) % INPUT_QUEUE_LENGTH;
        self.length += 1;
        assert!(self.length <= INPUT_QUEUE_LENGTH);
        self.first_frame = false;
        self.last_added_frame = frame_number;

        // We have been predicting. See if the inputs we've gotten match what we've been predicting. If so, don't worry about it.
        if self.prediction.frame != NULL_FRAME {
            assert!(frame_number == self.prediction.frame);

            // Remember the first input which was incorrect so we can report it
            if self.first_incorrect_frame == NULL_FRAME && !self.prediction.equal(&input, true) {
                self.first_incorrect_frame = frame_number;
            }

            // If this input is the same frame as the last one requested and we still haven't found any mispredicted inputs, we can exit prediction mode.
            // Otherwise, advance the prediction frame count up.
            if self.prediction.frame == self.last_requested_frame
                && self.first_incorrect_frame == NULL_FRAME
            {
                self.prediction.frame = NULL_FRAME;
            } else {
                self.prediction.frame += 1;
            }
        }
    }

    /// Advances the queue head to the next frame and either drops inputs or fills the queue if the input delay has changed since the last frame.
    fn advance_queue_head(&mut self, mut input_frame: Frame) -> Frame {
        let previous_position = match self.head {
            0 => INPUT_QUEUE_LENGTH - 1,
            _ => self.head - 1,
        };

        let mut expected_frame = if self.first_frame {
            0
        } else {
            self.inputs[previous_position].frame + 1
        };

        input_frame += self.frame_delay as i32;
        //  This can occur when the frame delay has dropped since the last time we shoved a frame into the system. In this case, there's no room on the queue. Toss it.
        if expected_frame > input_frame {
            return NULL_FRAME;
        }

        // This can occur when the frame delay has been increased since the last time we shoved a frame into the system.
        // We need to replicate the last frame in the queue several times in order to fill the space left.
        while expected_frame < input_frame {
            let input_to_replicate = self.inputs[previous_position];
            self.add_input_by_frame(input_to_replicate, expected_frame);
            expected_frame += 1;
        }

        let previous_position = match self.head {
            0 => INPUT_QUEUE_LENGTH - 1,
            _ => self.head - 1,
        };
        assert!(input_frame == 0 || input_frame == self.inputs[previous_position].frame + 1);
        input_frame
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod input_queue_tests {

    use std::net::SocketAddr;

    use serde::{Deserialize, Serialize};

    use super::*;

    #[repr(C)]
    #[derive(Copy, Clone, PartialEq, Default, Serialize, Deserialize)]
    struct TestInput {
        inp: u8,
    }

    struct TestConfig;

    impl Config for TestConfig {
        type Input = TestInput;
        type State = Vec<u8>;
        type Address = SocketAddr;
    }

    #[test]
    fn test_add_input_wrong_frame() {
        let mut queue = InputQueue::<TestConfig>::new();
        let input = PlayerInput::new(0, TestInput { inp: 0 });
        assert_eq!(queue.add_input(input), 0); // fine
        let input_wrong_frame = PlayerInput::new(3, TestInput { inp: 0 });
        assert_eq!(queue.add_input(input_wrong_frame), NULL_FRAME); // input dropped
    }

    #[test]
    fn test_add_input_twice() {
        let mut queue = InputQueue::<TestConfig>::new();
        let input = PlayerInput::new(0, TestInput { inp: 0 });
        assert_eq!(queue.add_input(input), 0); // fine
        assert_eq!(queue.add_input(input), NULL_FRAME); // input dropped
    }

    #[test]
    fn test_add_input_sequentially() {
        let mut queue = InputQueue::<TestConfig>::new();
        for i in 0..10 {
            let input = PlayerInput::new(i, TestInput { inp: 0 });
            queue.add_input(input);
            assert_eq!(queue.last_added_frame, i);
            assert_eq!(queue.length, (i + 1) as usize);
        }
    }

    #[test]
    fn test_input_sequentially() {
        let mut queue = InputQueue::<TestConfig>::new();
        for i in 0..10 {
            let input = PlayerInput::new(i, TestInput { inp: i as u8 });
            queue.add_input(input);
            assert_eq!(queue.last_added_frame, i);
            assert_eq!(queue.length, (i + 1) as usize);
            let (input_in_queue, _status) = queue.input(i);
            assert_eq!(input_in_queue.inp, i as u8);
        }
    }

    #[test]
    fn test_delayed_inputs() {
        let mut queue = InputQueue::<TestConfig>::new();
        let delay: i32 = 2;
        queue.set_frame_delay(delay as usize);
        for i in 0..10 {
            let input = PlayerInput::new(i, TestInput { inp: i as u8 });
            queue.add_input(input);
            assert_eq!(queue.last_added_frame, i + delay);
            assert_eq!(queue.length, (i + delay + 1) as usize);
            let (input_in_queue, _status) = queue.input(i);
            let correct_input = std::cmp::max(0, i - delay) as u8;
            assert_eq!(input_in_queue.inp, correct_input);
        }
    }
}
