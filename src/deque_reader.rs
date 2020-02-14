use std::io;
use std::pin::Pin;

use futures::future::poll_fn;
use futures::io::Error;
use futures::ready;
use futures::task::Context;
use futures::task::Poll;
use futures::AsyncReadExt;
use futures::{AsyncBufRead, AsyncRead};
use pin_project_lite::pin_project;
use slice_deque::SliceDeque;

pin_project! {
    pub struct DequeReader<R> {
        #[pin]
        inner: R,
        buf: SliceDeque<u8>,
    }
}

impl<R: AsyncRead> DequeReader<R> {
    fn new(inner: R) -> DequeReader<R> {
        Self::with_capacity(inner, 0)
    }

    fn with_capacity(inner: R, n: usize) -> DequeReader<R> {
        DequeReader {
            inner,
            buf: SliceDeque::with_capacity(n),
        }
    }

    fn poll_read_more(self: Pin<&mut Self>, cx: &mut Context) -> Poll<io::Result<()>> {
        let mut this = self.project();
        let mut buf = [0u8; 4096];
        let found = ready!(this.inner.poll_read(cx, &mut buf));
        let found = match found {
            Ok(n) => n,
            Err(e) => return Poll::Ready(Err(e)),
        };
        let buf = &buf[..found];
        this.buf.extend_from_slice(&buf);
        Poll::Ready(Ok(()))
    }
}

impl<R: Unpin + AsyncRead> DequeReader<R> {
    pub async fn read_more(&mut self) -> io::Result<()> {
        poll_fn(|cx| Pin::new(&mut *self).poll_read_more(cx)).await
    }
}

impl<R: Unpin + AsyncRead> AsyncRead for DequeReader<R> {
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

            let () = ready!(Pin::new(&mut *self).poll_read_more(cx)?);
        }

        let using = self.buf.len().min(buf.len());
        buf[..using].copy_from_slice(&self.buf.as_slice()[..using]);
        self.buf.drain(..using);

        Poll::Ready(Ok(using))
    }
}

impl<R: Unpin + AsyncRead> AsyncBufRead for DequeReader<R> {
    fn poll_fill_buf<'a>(
        mut self: Pin<&'a mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<&'a [u8], Error>> {
        if self.buf.is_empty() {
            let () = ready!(self.as_mut().poll_read_more(cx))?;
        }
        Poll::Ready(Ok(self.get_mut().buf.as_slice()))
    }

    fn consume(self: Pin<&mut Self>, amt: usize) {
        self.get_mut().buf.drain(..amt);
    }
}
