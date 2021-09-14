use crate::Frame;

const FRAME_WINDOW_SIZE: usize = 30;

#[derive(Debug)]
pub(crate) struct TimeSync {
    local: [i32; FRAME_WINDOW_SIZE],
    remote: [i32; FRAME_WINDOW_SIZE],
}

impl Default for TimeSync {
    fn default() -> Self {
        Self {
            local: [0; FRAME_WINDOW_SIZE],
            remote: [0; FRAME_WINDOW_SIZE],
        }
    }
}

impl TimeSync {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn advance_frame(&mut self, frame: Frame, local_adv: i32, remote_adv: i32) {
        self.local[frame as usize % self.local.len()] = local_adv;
        self.remote[frame as usize % self.remote.len()] = remote_adv;
    }

    pub(crate) fn average_frame_advantage(&self) -> i32 {
        // average local and remote frame advantages
        let local_sum: i32 = self.local.iter().sum();
        let local_avg = local_sum as f32 / self.local.len() as f32;
        let remote_sum: i32 = self.remote.iter().sum();
        let remote_avg = remote_sum as f32 / self.remote.len() as f32;

        // meet in the middle
        let sleep_frames = ((remote_avg - local_avg) / 2.0) as i32;

        sleep_frames
    }
}

// #########
// # TESTS #
// #########

#[cfg(test)]
mod sync_layer_tests {

    use super::*;

    #[test]
    fn test_advance_frame_no_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = 0;
            let remote_adv = 0;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.average_frame_advantage(), 0);
    }

    #[test]
    fn test_advance_frame_local_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = 5;
            let remote_adv = -5;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.average_frame_advantage(), -5);
    }

    #[test]
    fn test_advance_frame_small_remote_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = -1;
            let remote_adv = 1;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.average_frame_advantage(), 1);
    }

    #[test]
    fn test_advance_frame_remote_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = -4;
            let remote_adv = 4;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.average_frame_advantage(), 4);
    }

    #[test]
    fn test_advance_frame_big_remote_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = -40;
            let remote_adv = 40;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.average_frame_advantage(), 40);
    }
}
