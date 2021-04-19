use bytes::Bytes;
use ggez::{GGEZCallbacks, GGEZEvent};

struct Stub {
    pub state : u32
}

impl GGEZCallbacks for Stub {
    fn save_game_state(&mut self, buffer: &mut Bytes, frame: u32, checksum: Option<u32>) -> bool { true }

    fn load_game_state(&mut self, buffer: &Bytes) -> bool { true }

    fn log_game_state(&mut self, filename: String, buffer: &Bytes) -> bool { true }

    fn free_buffer(&mut self, buffer: &Bytes) { }

    fn advance_frame(&mut self) -> bool { true }

    fn on_event(&mut self, info: &GGEZEvent) { }
}

#[test]
fn test_start_session() {
    assert_eq!(2 + 2, 4);
}