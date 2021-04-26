use crate::game_info::GameInput;
use crate::{FrameNumber, GGEZError, PlayerHandle, INPUT_QUEUE_LENGTH, NULL_FRAME};
use std::cmp;

/// `InputQueue` handles inputs for a single player and saves them in a circular array. Valid Inputs are between `head` and `tail`.
#[derive(Debug, Copy, Clone)]
pub struct InputQueue {
    /// Identifies the player this InputQueue belongs to
    pub id: PlayerHandle,
    /// The head of the queue. The newest [GameInput] is here      
    head: usize,
    /// The tail of the queue. The oldest [GameInput] still valid is here.
    tail: usize,
    /// The current length of the queue.
    length: usize,
    /// Denotes if we still are in the first frame, an edge case to be considered by some methods.
    first_frame: bool,

    /// The last frame added by the user
    last_added_frame: FrameNumber,
    /// The first frame in the queue that is known to be an incorrect prediction
    first_incorrect_frame: FrameNumber,
    /// The last frame that has been requested. We make sure to never delete anything after this, as we would throw away important data.
    last_requested_frame: FrameNumber,

    /// The delay in frames by which inputs are sent back to the user. This can be set during initialization.
    frame_delay: u32,

    /// Our cyclic input queue
    inputs: [GameInput; INPUT_QUEUE_LENGTH],
    /// A pre-allocated prediction we are going to use to return predictions from.
    prediction: GameInput,
}

impl InputQueue {
    pub fn new(id: PlayerHandle, input_size: usize) -> InputQueue {
        InputQueue {
            id,
            head: 0,
            tail: 0,
            length: 0,
            frame_delay: 0,
            first_frame: true,
            last_added_frame: NULL_FRAME,
            first_incorrect_frame: NULL_FRAME,
            last_requested_frame: NULL_FRAME,

            prediction: GameInput::new(NULL_FRAME, None, input_size),
            inputs: [GameInput::new(NULL_FRAME, None, input_size); INPUT_QUEUE_LENGTH],
        }
    }

    pub fn get_last_confirmed_frame(&self) -> FrameNumber {
        self.last_added_frame
    }

    pub fn get_first_incorrect_frame(&self) -> FrameNumber {
        self.first_incorrect_frame
    }

    pub fn set_frame_delay(&mut self, delay: u32) {
        self.frame_delay = delay;
    }

    /// Resets all prediction errors back to `frame` or the `last_requested_frame`, whichever comes first.
    /// This is important to not throw away inputs that might still be necessary to synchronize.
    pub fn reset_prediction(&mut self, frame: FrameNumber) {
        assert!(self.first_incorrect_frame == NULL_FRAME || frame <= self.first_incorrect_frame);

        self.prediction.frame = NULL_FRAME;
        self.first_incorrect_frame = NULL_FRAME;
        self.last_requested_frame = NULL_FRAME;
    }

    /// Returns a [GameInput], but only if the input for the requested frame is confirmed
    pub fn get_confirmed_input(
        &self,
        requested_frame: FrameNumber,
    ) -> Result<GameInput, GGEZError> {
        // if we have recorded a first incorrect frame, the requested confirmed should be before that incorrect frame. We should never ask for such a frame.
        if self.first_incorrect_frame == NULL_FRAME || self.first_incorrect_frame > requested_frame
        {
            return Err(GGEZError::GeneralFailure(String::from(
                "InputQueue::get_confirmed_input(): The requested confirmed input is beyond a detected incorrect frame.",
            )));
        }

        let offset = requested_frame as usize % INPUT_QUEUE_LENGTH;

        if self.inputs[offset].frame == requested_frame {
            return Ok(self.inputs[offset]); // GameInput has copy semantics
        }
        Err(GGEZError::GeneralFailure(String::from(
            "InputQueue::get_confirmed_input(): The requested confirmed input could not be found",
        )))
    }

    /// Discards confirmed frames up to given `frame` from the queue. All confirmed frames are guaranteed to be synchronized between players, so there is no need to save the inputs anymore.
    pub fn discard_confirmed_frames(&mut self, mut frame: FrameNumber) {
        // we only drop frames until the last frame that was requested, otherwise we might delete data still needed
        if self.last_requested_frame != NULL_FRAME {
            frame = cmp::min(frame, self.last_requested_frame);
        }

        // move the tail to "delete inputs", wrap around if necessary
        if frame >= self.last_added_frame {
            self.tail = self.head;
        } else {
            let offset = (frame - (self.inputs[self.tail].frame)) as usize;
            self.tail = (self.tail + offset) % INPUT_QUEUE_LENGTH;
            self.length -= offset;
        }
    }

    /// Returns the game input of a single player for a given frame, if that input does not exist, we return a prediction instead.
    pub fn get_input(&mut self, requested_frame: FrameNumber) -> GameInput {
        // No one should ever try to grab any input when we have a prediction error.
        // Doing so means that we're just going further down the wrong path. Assert this to verify that it's true.
        assert!(self.first_incorrect_frame < 0);

        // Remember the last requested frame number for later. We'll need this in add_input() to drop out of prediction mode.
        self.last_requested_frame = requested_frame;

        assert!(requested_frame >= self.inputs[self.tail].frame);

        // We currently don't have a prediction frame
        if self.prediction.frame < 0 {
            //  If the frame requested is in our range, fetch it out of the queue and return it.
            let mut offset: usize = (requested_frame - self.inputs[self.tail].frame) as usize;

            if offset < self.length {
                offset = (offset + self.tail) % INPUT_QUEUE_LENGTH;
                assert!(self.inputs[offset].frame == requested_frame);
                return self.inputs[offset]; // GameInput has copy semantics
            }

            // The requested frame isn't in the queue. This means we need to return a prediction frame. Predict that the user will do the same thing they did last time.
            if requested_frame == 0 || self.last_added_frame == NULL_FRAME {
                // basing new prediction frame from nothing, since we are on frame 0 or we have no frames yet
                self.prediction.erase_bits();
            } else {
                // basing new prediction frame from previously added frame
                let previous_position: usize;
                match self.head {
                    0 => previous_position = INPUT_QUEUE_LENGTH - 1,
                    _ => previous_position = self.head - 1,
                }
                self.prediction = self.inputs[previous_position];
            }
            // update the prediction's frame
            self.prediction.frame += 1;
        }

        // We must be predicting, so we return the prediction frame contents. We are adjusting the prediction to have the requested frame.
        assert!(self.prediction.frame != NULL_FRAME);
        let mut prediction_to_return = self.prediction; // GameInput has copy semantics
        prediction_to_return.frame = requested_frame;
        return prediction_to_return;
    }

    /// Adds an input frame to the queue. Will consider the set frame delay.
    pub fn add_input(&mut self, input: &GameInput) {
        // These next two lines simply verify that inputs are passed in sequentially by the user, regardless of frame delay.
        assert!(self.last_added_frame == NULL_FRAME || input.frame == self.last_added_frame + 1);

        // Move the queue head to the correct point in preparation to input the frame into the queue.
        let new_frame = self.advance_queue_head(input.frame);
        // if the frame is valid, then add the input
        if new_frame != NULL_FRAME {
            self.add_input_by_frame(input, new_frame);
        }
    }

    /// Adds an input frame to the queue at the given frame number
    fn add_input_by_frame(&mut self, input: &GameInput, frame_number: FrameNumber) {
        let previous_position: usize;
        match self.head {
            0 => previous_position = INPUT_QUEUE_LENGTH - 1,
            _ => previous_position = self.head - 1,
        }

        assert!(input.size == self.prediction.size);
        assert!(self.last_added_frame == NULL_FRAME || frame_number == self.last_added_frame + 1);
        assert!(frame_number == 0 || self.inputs[previous_position].frame == frame_number - 1);

        // Add the frame to the back of the queue
        self.inputs[self.head] = input.clone();
        self.inputs[self.head].frame = frame_number;
        self.head = (self.head + 1) % INPUT_QUEUE_LENGTH;
        self.length += 1;
        assert!(self.length <= INPUT_QUEUE_LENGTH);
        self.first_frame = false;
        self.last_added_frame = frame_number;

        // We have been predicting. See if the inputs we've gotten match what we've been predicting. If so, don't worry about it.
        // If not, remember the first input which was incorrect so we can report it in GetFirstIncorrectFrame()
        if self.prediction.frame != NULL_FRAME {
            assert!(frame_number == self.prediction.frame);

            // Remember the first input which was incorrect so we can report it in GetFirstIncorrectFrame()
            if self.first_incorrect_frame == NULL_FRAME && !self.prediction.equal(&input, true) {
                self.first_incorrect_frame = frame_number;
            }

            // If this input is the same frame as the last one requested and we still haven't found any mispredicted inputs, we can exit predition mode.
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
    fn advance_queue_head(&mut self, mut input_frame: FrameNumber) -> FrameNumber {
        let previous_position: usize;
        match self.head {
            0 => previous_position = INPUT_QUEUE_LENGTH - 1,
            _ => previous_position = self.head - 1,
        }

        let mut expected_frame: FrameNumber;
        match self.first_frame {
            true => expected_frame = 0,
            false => expected_frame = self.inputs[previous_position].frame + 1,
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
            self.add_input_by_frame(&input_to_replicate, expected_frame);
            expected_frame += 1;
        }

        assert!(input_frame == 0 || input_frame == self.inputs[previous_position].frame + 1);
        input_frame
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod input_queue_tests {

    use super::*;

    #[test]
    #[should_panic]
    fn test_add_input_wrong_frame() {
        let mut queue = InputQueue::new(0, std::mem::size_of::<u32>());
        let input = GameInput::new(3, None, std::mem::size_of::<u32>());
        queue.add_input(&input); // because input has a wrong frame, this panics
    }

    #[test]
    #[should_panic]
    fn test_add_input_twice() {
        let mut queue = InputQueue::new(0, std::mem::size_of::<u32>());
        let input = GameInput::new(0, None, std::mem::size_of::<u32>());
        queue.add_input(&input); // fine
        queue.add_input(&input); // not fine
    }

    #[test]
    fn test_add_input_sequentally() {
        let mut queue = InputQueue::new(0, std::mem::size_of::<u32>());
        for i in 0..10 {
            let input = GameInput::new(i, None, std::mem::size_of::<u32>());
            queue.add_input(&input);
            assert_eq!(queue.last_added_frame, i);
            assert_eq!(queue.length, (i + 1) as usize);
        }
    }

    #[test]
    fn test_get_input_sequentally() {
        let mut queue = InputQueue::new(0, std::mem::size_of::<u32>());
        for i in 0..10 {
            let input = GameInput::new(i, None, std::mem::size_of::<u32>());
            queue.add_input(&input);
            assert_eq!(queue.last_added_frame, i);
            assert_eq!(queue.length, (i + 1) as usize);
            let input_in_queue = queue.get_input(i);
            assert!(input_in_queue.equal(&input, false));
        }
    }
}
