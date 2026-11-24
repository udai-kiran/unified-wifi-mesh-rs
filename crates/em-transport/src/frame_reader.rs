// SPDX-License-Identifier: Apache-2.0

//! Reads length-prefixed frames from a stream, so that a read cancelled by `select!` loses nothing.

use std::io;

use tokio::io::{AsyncRead, AsyncReadExt};

use crate::wire::{LENGTH_PREFIX_LEN, MAX_FRAME_LEN};

/// Octets asked from the stream per read.
const READ_CHUNK: usize = 16 * 1024;

/// One thing read from the stream.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Frame {
    /// A frame body, without its length prefix.
    Complete(Vec<u8>),
    /// A frame longer than [`MAX_FRAME_LEN`]. Its whole body was read and thrown away,
    /// so the stream is at the start of the next frame (0011 D3 step 6).
    Oversize { length: u32 },
}

/// Reads frames from `reader`.
///
/// Octets that arrive are kept in `buffer`, and `read` is the only await point that
/// waits for the stream. Dropping the future of [`FrameReader::next_frame`] between
/// polls therefore loses no data.
pub(crate) struct FrameReader<R> {
    reader: R,
    buffer: Vec<u8>,
    /// Where one read lands. On the heap so that the futures stay small.
    chunk: Box<[u8]>,
    /// Octets of an oversize body still to throw away.
    skip_remaining: u64,
    /// Length of the oversize frame being skipped.
    skipping_length: u32,
}

impl<R: AsyncRead + Unpin> FrameReader<R> {
    pub(crate) fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Vec::new(),
            chunk: vec![0; READ_CHUNK].into_boxed_slice(),
            skip_remaining: 0,
            skipping_length: 0,
        }
    }

    /// Waits for the next frame. End of file, also in the middle of a frame, is
    /// `ErrorKind::UnexpectedEof`.
    ///
    /// Cancel safe.
    pub(crate) async fn next_frame(&mut self) -> io::Result<Frame> {
        loop {
            if let Some(frame) = self.take_frame() {
                return Ok(frame);
            }
            let count = self.reader.read(&mut self.chunk).await?;
            if count == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            self.buffer.extend_from_slice(&self.chunk[..count]);
        }
    }

    /// Takes one frame from the buffer if it holds one.
    fn take_frame(&mut self) -> Option<Frame> {
        loop {
            if self.skip_remaining > 0 {
                // A remaining count too big for usize is certainly above the buffer length.
                let discard = usize::try_from(self.skip_remaining)
                    .map_or(self.buffer.len(), |remaining| {
                        remaining.min(self.buffer.len())
                    });
                self.buffer.drain(..discard);
                self.skip_remaining -= discard as u64;
                if self.skip_remaining > 0 {
                    return None;
                }
                return Some(Frame::Oversize {
                    length: self.skipping_length,
                });
            }

            let prefix: [u8; LENGTH_PREFIX_LEN] =
                self.buffer.get(..LENGTH_PREFIX_LEN)?.try_into().ok()?;
            let length = u32::from_be_bytes(prefix);
            if length > MAX_FRAME_LEN {
                self.buffer.drain(..LENGTH_PREFIX_LEN);
                self.skip_remaining = u64::from(length);
                self.skipping_length = length;
                continue;
            }

            let end = LENGTH_PREFIX_LEN + usize::try_from(length).ok()?;
            if self.buffer.len() < end {
                return None;
            }
            let body = self.buffer[LENGTH_PREFIX_LEN..end].to_vec();
            self.buffer.drain(..end);
            return Some(Frame::Complete(body));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncWriteExt, DuplexStream};
    use tokio::task::JoinHandle;

    use super::*;

    /// A broken reader waits for octets that never come, or loops on end of file. The
    /// tests run in real time, so that this limit also ends a loop that reads without waiting.
    const LIMIT: Duration = Duration::from_secs(10);

    async fn next_frame(frames: &mut FrameReader<DuplexStream>) -> io::Result<Frame> {
        tokio::time::timeout(LIMIT, frames.next_frame())
            .await
            .expect("the frame reader stalled")
    }

    /// Writes `bytes` from a task, so that a body larger than the pipe does not block the test.
    fn write_in_background(mut writer: DuplexStream, bytes: Vec<u8>) -> JoinHandle<DuplexStream> {
        tokio::spawn(async move {
            writer.write_all(&bytes).await.unwrap();
            writer
        })
    }

    async fn finish_writing(writing: JoinHandle<DuplexStream>) {
        let writer = tokio::time::timeout(LIMIT, writing)
            .await
            .expect("the writer stalled")
            .unwrap();
        drop(writer);
    }

    #[tokio::test]
    async fn reads_frames_split_across_reads() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let mut frames = FrameReader::new(reader);
        writer.write_all(&[0, 0, 0, 3, 1, 2]).await.unwrap();
        writer.write_all(&[3, 0, 0, 0, 1, 9]).await.unwrap();
        assert_eq!(
            next_frame(&mut frames).await.unwrap(),
            Frame::Complete(vec![1, 2, 3])
        );
        assert_eq!(
            next_frame(&mut frames).await.unwrap(),
            Frame::Complete(vec![9])
        );
    }

    #[tokio::test]
    async fn oversize_frame_is_skipped_and_the_next_one_is_in_step() {
        let (writer, reader) = tokio::io::duplex(1024);
        let mut frames = FrameReader::new(reader);
        let oversize = MAX_FRAME_LEN + 1;
        let mut bytes = oversize.to_be_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(0xAA, oversize as usize));
        bytes.extend_from_slice(&[0, 0, 0, 1, 7]);
        let writing = write_in_background(writer, bytes);
        assert_eq!(
            next_frame(&mut frames).await.unwrap(),
            Frame::Oversize { length: oversize }
        );
        assert_eq!(
            next_frame(&mut frames).await.unwrap(),
            Frame::Complete(vec![7])
        );
        finish_writing(writing).await;
    }

    #[tokio::test]
    async fn frame_of_exactly_the_maximum_length_is_read() {
        let (writer, reader) = tokio::io::duplex(1024);
        let mut frames = FrameReader::new(reader);
        let mut bytes = MAX_FRAME_LEN.to_be_bytes().to_vec();
        bytes.extend(std::iter::repeat_n(5, MAX_FRAME_LEN as usize));
        let writing = write_in_background(writer, bytes);
        let Frame::Complete(body) = next_frame(&mut frames).await.unwrap() else {
            panic!("expected a complete frame");
        };
        assert_eq!(body.len(), MAX_FRAME_LEN as usize);
        finish_writing(writing).await;
    }

    #[tokio::test]
    async fn limit_is_65536_octets_not_one_more_or_less() {
        // Literals, so that a change of `MAX_FRAME_LEN` itself is noticed.
        let (writer, reader) = tokio::io::duplex(1024);
        let mut frames = FrameReader::new(reader);
        let mut bytes = Vec::new();
        for length in [65_536u32, 65_537] {
            bytes.extend_from_slice(&length.to_be_bytes());
            bytes.extend(std::iter::repeat_n(1, length as usize));
        }
        bytes.extend_from_slice(&[0, 0, 0, 1, 7]);
        let writing = write_in_background(writer, bytes);

        let Frame::Complete(body) = next_frame(&mut frames).await.unwrap() else {
            panic!("65 536 octets must be delivered");
        };
        assert_eq!(body.len(), 65_536);
        assert_eq!(
            next_frame(&mut frames).await.unwrap(),
            Frame::Oversize { length: 65_537 }
        );
        assert_eq!(
            next_frame(&mut frames).await.unwrap(),
            Frame::Complete(vec![7])
        );
        finish_writing(writing).await;
    }

    #[tokio::test]
    async fn end_of_file_inside_a_frame_is_unexpected_eof() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let mut frames = FrameReader::new(reader);
        writer.write_all(&[0, 0, 0, 5, 1]).await.unwrap();
        drop(writer);
        let error = next_frame(&mut frames).await.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
}
