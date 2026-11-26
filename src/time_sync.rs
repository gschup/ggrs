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
        // NULL_FRAME and other negative frames are not valid time-sync samples;
        // casting them to usize would land on an unrelated window slot.
        if frame < 0 {
            return;
        }
        self.local[frame as usize % self.local.len()] = local_adv;
        self.remote[frame as usize % self.remote.len()] = remote_adv;
    }

    pub(crate) fn average_frame_advantage(&self) -> i32 {
        // Average local and remote frame advantages. Sum in i64: each sample
        // can be near i32::MAX after RTT saturation, and 30 of them overflow
        // i32 in debug builds.
        let local_sum: i64 = self.local.iter().map(|&value| value as i64).sum();
        let local_avg = local_sum as f64 / self.local.len() as f64;
        let remote_sum: i64 = self.remote.iter().map(|&value| value as i64).sum();
        let remote_avg = remote_sum as f64 / self.remote.len() as f64;

        // meet in the middle
        ((remote_avg - local_avg) / 2.0) as i32
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

    #[test]
    fn test_extreme_advantages_do_not_overflow_average() {
        let mut time_sync = TimeSync::default();
        for frame in 0..FRAME_WINDOW_SIZE {
            time_sync.advance_frame(frame as Frame, i32::MAX, i32::MIN);
        }
        // The exact value is not important; it must not panic or saturate
        // unpredictably.
        let _ = time_sync.average_frame_advantage();
    }

    #[test]
    fn test_negative_frame_is_ignored() {
        let mut time_sync = TimeSync::default();
        time_sync.advance_frame(crate::NULL_FRAME, 100, 100);
        assert_eq!(time_sync.average_frame_advantage(), 0);
    }

    #[test]
    fn test_window_wraparound_uses_only_recent_frames() {
        // FRAME_WINDOW_SIZE is 30. Fill 60 frames: first 30 with advantage 10,
        // second 30 (which overwrite them) with advantage 0. Result should be 0.
        let mut time_sync = TimeSync::default();
        for i in 0..30 {
            time_sync.advance_frame(i, -10, 10); // advantage = 10
        }
        for i in 30..60 {
            time_sync.advance_frame(i, 0, 0); // advantage = 0, overwrites slots
        }
        assert_eq!(time_sync.average_frame_advantage(), 0);
    }

    #[test]
    fn test_partial_window_includes_zero_initialized_slots() {
        // Only fill 10 of 30 slots with advantage 30 (local=-30, remote=30).
        // The other 20 slots are zero-initialized.
        // Expected: ((10 * 30 + 20 * 0) / 30 - (10 * -30 + 20 * 0) / 30) / 2
        //         = (10 - (-10)) / 2 = 10
        let mut time_sync = TimeSync::default();
        for i in 0..10 {
            time_sync.advance_frame(i, -30, 30);
        }
        assert_eq!(time_sync.average_frame_advantage(), 10);
    }

    #[test]
    fn test_asymmetric_advantages() {
        // local_adv=2, remote_adv=6 → (6/1 - 2/1) / 2 = 2 (full window, all same)
        let mut time_sync = TimeSync::default();
        for i in 0..30 {
            time_sync.advance_frame(i, 2, 6);
        }
        assert_eq!(time_sync.average_frame_advantage(), 2);
    }
}
