mod stubs;

#[test]
fn test_create_session() {
    ggrs::start_synctest_session(2, std::mem::size_of::<u32>(), 1).unwrap();
}
