use std::fmt;

use tokio::sync::{mpsc, oneshot};

use super::image_encoder::SharedRgbaImage;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewCaptureError {
    /// Too many captures are already waiting for a frame.
    Busy,
    /// The view has no single image to return. The text explains why.
    Unavailable(String),
    /// The capture started but could not finish.
    Failed(String),
}

impl fmt::Display for ViewCaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("too many view captures are waiting"),
            Self::Unavailable(reason) | Self::Failed(reason) => f.write_str(reason),
        }
    }
}

pub type ViewCaptureResult = Result<SharedRgbaImage, ViewCaptureError>;

/// One waiting caller. It receives the image when a frame has been captured.
pub type ViewCaptureReply = oneshot::Sender<ViewCaptureResult>;

/// Create the two ends used to ask the render loop for a view image.
pub fn view_capture_channel(capacity: usize) -> (ViewCaptureClient, ViewCaptureQueue) {
    let (requests, receiver) = mpsc::channel(capacity);
    (ViewCaptureClient { requests }, ViewCaptureQueue { receiver })
}

/// Used by any thread to request the current view image.
#[derive(Clone)]
pub struct ViewCaptureClient {
    requests: mpsc::Sender<ViewCaptureReply>,
}

impl ViewCaptureClient {
    /// Wait until the render loop returns an image.
    pub async fn capture(&self) -> ViewCaptureResult {
        let (reply, result) = oneshot::channel();
        self.requests.try_send(reply).map_err(|err| match err {
            mpsc::error::TrySendError::Full(_) => ViewCaptureError::Busy,
            mpsc::error::TrySendError::Closed(_) => stopped_error(),
        })?;
        result.await.unwrap_or_else(|_| Err(stopped_error()))
    }
}

fn stopped_error() -> ViewCaptureError {
    ViewCaptureError::Failed("render loop stopped".to_owned())
}

/// Owned by the render loop, which checks it once per frame.
pub struct ViewCaptureQueue {
    receiver: mpsc::Receiver<ViewCaptureReply>,
}

impl ViewCaptureQueue {
    /// Return whether a caller is waiting for an image.
    pub fn has_pending(&self) -> bool {
        !self.receiver.is_empty()
    }

    /// Take all waiting callers so one capture can answer them together.
    pub fn take_pending(&mut self) -> Vec<ViewCaptureReply> {
        let mut replies = Vec::new();
        while let Ok(reply) = self.receiver.try_recv() {
            replies.push(reply);
        }
        replies
    }
}

/// Send the same result to every waiting caller.
pub fn respond_all(replies: Vec<ViewCaptureReply>, result: ViewCaptureResult) {
    for reply in replies {
        // A caller that timed out has dropped its receiver, so this may fail.
        let _ = reply.send(result.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn one_capture_answers_every_waiting_caller() {
        let (client, mut queue) = view_capture_channel(2);
        let (first, second) = (client.clone(), client.clone());
        let callers = tokio::spawn(async move {
            tokio::join!(first.capture(), second.capture())
        });

        let mut replies = Vec::new();
        while replies.len() < 2 {
            replies.extend(queue.take_pending());
            tokio::task::yield_now().await;
        }
        let image = SharedRgbaImage::from_raw(1, 1, vec![0; 4].into()).unwrap();
        respond_all(replies, Ok(image));
        assert!(!queue.has_pending());

        let (first, second) = callers.await.unwrap();
        assert!(first.is_ok() && second.is_ok());
    }

    #[tokio::test]
    async fn full_or_closed_queue_returns_an_error() {
        let (client, queue) = view_capture_channel(1);
        let waiting = client.clone();
        let waiting = tokio::spawn(async move { waiting.capture().await });
        while client.requests.capacity() > 0 {
            tokio::task::yield_now().await;
        }
        assert_eq!(client.capture().await.unwrap_err(), ViewCaptureError::Busy);

        // Dropping the queue must wake the waiting caller.
        drop(queue);
        assert_eq!(waiting.await.unwrap().unwrap_err(), stopped_error());
        assert_eq!(client.capture().await.unwrap_err(), stopped_error());
    }
}
