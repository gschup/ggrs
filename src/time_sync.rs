use crate::Frame;

const FRAME_WINDOW_SIZE: usize = 30;
const MIN_FRAME_ADVANTAGE: i32 = 0;
const MAX_FRAME_ADVANTAGE: i32 = 8;

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

    pub(crate) fn recommend_frame_delay(&self) -> u32 {
        // average local and remote frame advantages
        let local_sum: i32 = self.local.iter().sum();
        let local_avg = local_sum as f32 / self.local.len() as f32;
        let remote_sum: i32 = self.remote.iter().sum();
        let remote_avg = remote_sum as f32 / self.remote.len() as f32;

        // if we have the advantage, we are behind and don't need to wait.
        if local_avg >= remote_avg {
            return 0;
        }

        // meet in the middle
        let sleep_frames = (((remote_avg - local_avg) / 2.0_f32) + 0.5) as i32;

        // only wait if the discrepancy is big enough
        if sleep_frames < MIN_FRAME_ADVANTAGE {
            return 0;
        }

        // never recommend beyond maximum wait (this should never happen anyway)
        std::cmp::min(sleep_frames, MAX_FRAME_ADVANTAGE) as u32
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

        assert_eq!(time_sync.recommend_frame_delay(), 0);
    }

    #[test]
    fn test_advance_frame_local_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = 5;
            let remote_adv = -5;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.recommend_frame_delay(), 0);
    }

    #[test]
    fn test_advance_frame_small_remote_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = -1;
            let remote_adv = 1;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.recommend_frame_delay(), 1);
    }

    #[test]
    fn test_advance_frame_remote_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = -4;
            let remote_adv = 4;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.recommend_frame_delay(), 4);
    }

    #[test]
    fn test_advance_frame_big_remote_advantage() {
        let mut time_sync = TimeSync::default();

        for i in 0..60 {
            let local_adv = -40;
            let remote_adv = 40;
            time_sync.advance_frame(i, local_adv, remote_adv)
        }

        assert_eq!(time_sync.recommend_frame_delay(), 8);
    }
}
