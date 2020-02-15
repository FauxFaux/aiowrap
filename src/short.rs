use std::io;

use futures::task::Context;
use futures::task::Poll;
use futures::AsyncRead;
use pin_project_lite::pin_project;
use std::pin::Pin;

pin_project! {
    /// Intentionally return short reads, to test `AsyncRead` code.
    ///
    /// The `decider` iterator gets to decide how short a read should be.
    /// A read length of 0 generates an `Poll::Pending`, with an immediate wakeup.
    /// When the iterator runs out before the reader, `read` will always
    /// return zero-length reads (EOF).
    ///
    /// Currently, no effort is made to make reads longer, if the underlying
    /// reader naturally returns short reads.
    ///
    /// # Examples
    ///
    /// Short read:
    ///
    /// ```rust
    /// use futures::io;
    /// use futures::io::AsyncReadExt as _;
    /// # use async_std::task;
    /// # task::block_on(async {
    /// let mut naughty = aiowrap::ShortRead::new(
    ///         io::Cursor::new(b"1234567890"),
    ///         vec![2, 3, 4, 5, 6].into_iter()
    /// );
    /// let mut buf = [0u8; 10];
    /// // A `Cursor` would normally return the whole ten bytes here,
    /// // but we've limited it to two bytes.
    /// assert_eq!(2, naughty.read(&mut buf).await.unwrap());
    /// # });
    /// ```
    pub struct ShortRead<R, I> {
        #[pin]
        inner: R,
        decider: I,
    }
}

impl<R: AsyncRead, I: Iterator<Item = usize>> AsyncRead for ShortRead<R, I> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.project();
        let wanted = match this.decider.next() {
            Some(0) => {
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Some(wanted) => wanted,
            None => return Poll::Ready(Ok(0)),
        };
        let wanted = wanted.min(buf.len());

        let buf = &mut buf[..wanted];
        this.inner.poll_read(cx, buf)
    }
}

impl<R, I: Iterator<Item = usize>> ShortRead<R, I> {
    pub fn new(inner: R, decider: I) -> Self {
        ShortRead { inner, decider }
    }

    pub fn into_inner(self) -> R {
        self.inner
    }
}

#[cfg(test)]
mod tests {
    use crate::ShortRead;

    use async_std::task;
    use futures::io;
    use futures::io::AsyncReadExt as _;

    #[test]
    fn shorten() {
        task::block_on(async {
            let mut naughty = ShortRead::new(
                io::Cursor::new(b"1234567890"),
                vec![2, 3, 4, 5, 6].into_iter(),
            );
            let mut buf = [0u8; 10];
            assert_eq!(2, naughty.read(&mut buf).await.unwrap());
            assert_eq!(b"12", &buf[..2]);
            assert_eq!(3, naughty.read(&mut buf).await.unwrap());
            assert_eq!(b"345", &buf[..3]);
            assert_eq!(4, naughty.read(&mut buf).await.unwrap());
            assert_eq!(b"6789", &buf[..4]);
            assert_eq!(1, naughty.read(&mut buf).await.unwrap());
            assert_eq!(b"0", &buf[..1]);

            assert_eq!(0, naughty.read(&mut buf).await.unwrap());
            assert_eq!(0, naughty.read(&mut buf).await.unwrap());
        });
    }

    #[test]
    fn interrupt() {
        task::block_on(async {
            let mut interrupting =
                ShortRead::new(io::Cursor::new(b"12"), vec![0, 1, 0, 1].into_iter());
            let mut buf = [0; 1];

            assert_eq!(1, interrupting.read(&mut buf).await.unwrap());
            assert_eq!(1, interrupting.read(&mut buf).await.unwrap());
        });
    }
}
