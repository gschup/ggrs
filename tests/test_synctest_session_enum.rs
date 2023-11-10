mod stubs_enum;

use ggrs::{GgrsError, SessionBuilder};

#[test]
fn test_enum_advance_frames_with_delayed_input() -> Result<(), GgrsError> {
    let check_distance = 7;
    let mut stub = stubs_enum::GameStubEnum::new();
    let mut sess = SessionBuilder::new()
        .with_check_distance(check_distance)
        .with_input_delay(2)
        .start_synctest_session()?;

    let inputs = [stubs_enum::EnumInput::Val1, stubs_enum::EnumInput::Val2];
    for i in 0..200 {
        let input = inputs[i % inputs.len()];
        sess.add_local_input(0, input)?;
        sess.add_local_input(1, input)?;
        let requests = sess.advance_frame()?;
        stub.handle_requests(requests);
        assert_eq!(stub.gs.frame, i as i32 + 1); // frame should have advanced
    }

    Ok(())
}
