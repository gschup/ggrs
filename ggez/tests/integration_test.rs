use adler::Adler32;
use bincode;
use ggez::{GGEZEvent, GGEZInterface};
use std::hash::Hash;

#[derive(Hash)]
struct Stub {
    pub state: u32,
}

impl GGEZInterface for Stub {
    fn save_game_state(&self, buffer: &mut Vec<u8>, checksum: &mut Option<u32>) {
        *buffer = bincode::serialize(&self.state).unwrap();

        let mut adler = Adler32::new();
        self.hash(&mut adler);
        *checksum = Some(adler.checksum());
    }

    fn load_game_state(&mut self, buffer: &[u8]) {
        self.state = bincode::deserialize(buffer).unwrap();
    }

    fn advance_frame(&mut self) {}

    fn on_event(&mut self, info: &GGEZEvent) {
        println!("{:?}", info);
    }
}

#[test]
fn test_start_session() {
    let _stub = Stub { state: 5 };
    assert_eq!(2 + 2, 4);
}
