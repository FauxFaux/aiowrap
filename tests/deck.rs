use aiowrap::DequeReader;
use aiowrap::ShortRead;
use async_std::task;
use futures::io;

#[test]
fn smoke() {
    task::block_on(async {
        let mut m = DequeReader::new(ShortRead::new(
            io::Cursor::new(b"hello world"),
            vec![2, 3, 4, 5].into_iter(),
        ));
        assert_eq!(b"", m.buffer());
        m.read_more().await.unwrap();
        assert_eq!(b"he", m.buffer());
        m.read_more().await.unwrap();
        assert_eq!(b"hello", m.buffer());
    });
}
