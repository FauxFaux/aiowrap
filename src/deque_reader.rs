use std::io;
use std::pin::Pin;

use futures::future::poll_fn;
use futures::io::IoSlice;
use futures::ready;
use futures::task::Context;
use futures::task::Poll;
use futures::AsyncBufRead;
use futures::AsyncRead;
use futures::AsyncWrite;
use pin_project_lite::pin_project;
use slice_deque::SliceDeque;

pin_project! {
    /// An interface like `io::BufReader`, but extra data can be *repeatedly* added.
    ///
    /// ```
    /// # use std::pin::Pin;
    /// # use async_std::task;
    /// # use futures::io;
    /// # use futures::io::AsyncBufRead as _;
    /// # use aiowrap::DequeReader;
    /// # async_std::task::block_on(async {
    /// let mut m = DequeReader::new(io::Cursor::new(b"hello world"));
    /// // gather more and more data
    /// while m.read_more().await.expect("no io errors") {
    ///     // until we find an 'r'
    ///     if let Some(r) = m.buffer().iter().position(|&c| c == b'r') {
    ///         // then discard everything up to that point
    ///         Pin::new(&mut m).consume(r);
    ///         return;
    ///     }
    /// }
    /// panic!("reached eof")
    /// # });
    /// ```
    pub struct DequeReader<R> {
        #[pin]
        inner: R,
        buf: SliceDeque<u8>,
    }
}

impl<R> DequeReader<R> {
    /// Wrap a reader, without allocating a buffer. The buffer will be allocated, and grown, on use.
    pub fn new(inner: R) -> DequeReader<R> {
        Self::with_capacity(inner, 0)
    }

    /// Wrap a reader, pre-allocating a buffer of a specific size. The buffer will be grown on use.
    ///
    /// Note: [SliceDeque] has stringent, platform dependent rules
    /// around the buffer size, so the resulting buffer size may be wildly different.
    pub fn with_capacity(inner: R, n: usize) -> DequeReader<R> {
        DequeReader {
            inner,
            buf: SliceDeque::with_capacity(n),
        }
    }

    /// Gets a reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    pub fn get_ref(&self) -> &R {
        &self.inner
    }

    /// Gets a mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    pub fn get_mut(&mut self) -> &mut R {
        &mut self.inner
    }

    /// Gets a pinned mutable reference to the underlying reader.
    ///
    /// It is inadvisable to directly read from the underlying reader.
    pub fn get_pin_mut(self: Pin<&mut Self>) -> Pin<&mut R> {
        self.project().inner
    }

    /// Consumes this, returning the underlying reader.
    ///
    /// Note that any leftover data in the internal buffer is lost.
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: AsyncRead> DequeReader<R> {
    /// Attempt a large read against the `inner` reader.
    ///
    /// If a byte could not be read as we are at the end of the stream, return `false`.
    pub fn poll_read_more(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<bool>> {
        let this = self.project();
        let mut buf = [0u8; 4096];
        let found = ready!(this.inner.poll_read(cx, &mut buf));
        let found = match found {
            Ok(n) => n,
            Err(e) => return Poll::Ready(Err(e)),
        };
        let buf = &buf[..found];
        this.buf.extend_from_slice(&buf);
        Poll::Ready(Ok(!buf.is_empty()))
    }

    /// Access the inner buffer directly, without attempting any reads.
    pub fn buffer(&self) -> &[u8] {
        self.buf.as_slice()
    }
}

impl<R: Unpin + AsyncRead> DequeReader<R> {
    /// Resolves when we can read at least one extra byte into the inner reader,
    /// typically many more, returning `true` until we are at eof.
    pub async fn read_more(&mut self) -> io::Result<bool> {
        // surely there's a more elegant way to write this
        poll_fn(|cx| Pin::new(&mut *self).poll_read_more(cx)).await
    }
}

impl<R: AsyncRead> AsyncRead for DequeReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }

        if self.buf.is_empty() {
            if buf.len() >= 4 * 1024 {
                let this = self.project();
                return this.inner.poll_read(cx, buf);
            }

            let _any_more = ready!(self.as_mut().poll_read_more(cx)?);
        }

        let using = self.buf.len().min(buf.len());
        buf[..using].copy_from_slice(&self.buf.as_slice()[..using]);

        let this = self.project();
        this.buf.drain(..using);

        Poll::Ready(Ok(using))
    }
}

impl<R: AsyncRead> AsyncBufRead for DequeReader<R> {
    fn poll_fill_buf<'a>(
        mut self: Pin<&'a mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<&'a [u8]>> {
        if self.buf.is_empty() {
            let _any_more = ready!(self.as_mut().poll_read_more(cx))?;
        }
        let this = self.project();
        Poll::Ready(Ok(this.buf.as_slice()))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        let this = self.project();
        this.buf.drain(..amt);
    }
}

impl<W: AsyncWrite> AsyncWrite for DequeReader<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write(cx, buf)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        self.project().inner.poll_write_vectored(cx, bufs)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_flush(cx)
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.project().inner.poll_close(cx)
    }
}

#[cfg(test)]
mod test {
    use std::pin::Pin;

    use async_std::task;
    use futures::io;
    use futures::io::AsyncBufRead;

    use crate::DequeReader;
    use crate::ShortRead;

    #[test]
    fn buf_read() {
        task::block_on(async {
            let mut m = DequeReader::new(ShortRead::new(
                io::Cursor::new(b"hello world"),
                vec![2, 3, 4, 5].into_iter(),
            ));
            assert_eq!(b"", m.buffer());
            assert_eq!(true, m.read_more().await.unwrap());
            assert_eq!(b"he", m.buffer());
            assert_eq!(true, m.read_more().await.unwrap());
            assert_eq!(b"hello", m.buffer());

            Pin::new(&mut m).consume(2);
            assert_eq!(b"llo", m.buffer());

            assert_eq!(true, m.read_more().await.unwrap());
            assert_eq!(b"llo wor", m.buffer());

            Pin::new(&mut m).consume(1);
            assert_eq!(b"lo wor", m.buffer());

            assert_eq!(true, m.read_more().await.unwrap());
            assert_eq!(b"lo world", m.buffer());

            assert_eq!(false, m.read_more().await.unwrap());
            assert_eq!(b"lo world", m.buffer());
            assert_eq!(false, m.read_more().await.unwrap());
        });
    }
}
