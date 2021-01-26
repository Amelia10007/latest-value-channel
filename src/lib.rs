//! Multi-producer, single-consumer ***SINGLE*** data communication primitives.
//!
//! This crate provides a latest-message style [`channel`](fn.channel.html),
//! where [`Updater`](struct.Updater.html)(s) can update the latest data that the [`Receiver`](struct.Receiver.html) owns it.
//!
//! Unlike the `std::sync::mpsc::channel`, by using `channel` of this crate, each data send will overwrite the original data.
//! Once the `receiver` receives the data of its channel, `receiver` can retrieve nothing unless the updater(s) updates the data.
//! These property are useful, for example, when a thread is interested in the ***latest*** result of another continually working thread.
//!
//! # Examples
//! ## Basic usage
//! ```
//! use latest_value_channel::channel;
//!
//! let (updater, receiver) = channel();
//!
//! std::thread::spawn(move || {
//!     updater.update(1).unwrap();
//! })
//! .join().unwrap();
//!
//! assert_eq!(Ok(1), receiver.recv());
//! ```
//! ## Multiple update
//! ```
//! use latest_value_channel::channel;
//!
//! let (updater, receiver) = channel();
//! let updater2 = updater.clone(); // updater can be cloned.
//!
//! // Only the latest data will be received
//! updater.update(1).unwrap();
//! updater.update(2).unwrap();
//! assert_eq!(Ok(2), receiver.recv());
//!
//! updater.update(10).unwrap();
//! updater2.update(20).unwrap();
//! assert_eq!(Ok(20), receiver.recv());
//!
//! // Channel is valid as long as at least 1 updater exists.
//! drop(updater);
//! updater2.update(200).unwrap();
//! assert_eq!(Ok(200), receiver.recv());
//! ```
//! ## Receive after updater dropped
//! ```
//! use latest_value_channel::channel;
//!
//! let (updater, receiver) = channel::<i32>();
//!
//! drop(updater);
//! // The updater dropped and no data exists on the buffer. So recv() fails.
//! assert!(receiver.recv().is_err());
//! ```
//! ## Receive after update and updater dropped
//! ```
//! use latest_value_channel::channel;
//!
//! let (updater, receiver) = channel();
//!
//! updater.update(1).unwrap();
//! drop(updater);
//!
//! // The updater dropped. but the updated data remains, so recv() succeeds.
//! assert_eq!(Ok(1), receiver.recv());
//!
//! // The updater dropped and no data exists on the buffer. So recv() fails.
//! assert!(receiver.recv().is_err());
//! ```

use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::{Arc, Condvar, Mutex, Weak};

/// The receiving-half of the single data channel.
///
/// Latest data of the channel can be retrieved using `recv` and `try_recv`.
#[derive(Debug)]
pub struct Receiver<T> {
    /// Stores the latest data as `Some(data)`, or `None` if updater(s) has not call its `update` yet.
    buffer: Arc<Mutex<Option<T>>>,
    /// Receives notifications that the data is updated during wait.
    condvar: Arc<Condvar>,
}

impl<T> Receiver<T> {
    /// Waits the latest data of this channel. This method blocks the current thread.
    /// Little CPU time is consumed during the block.
    ///
    /// After successful reception, nothing can be retrieved unless the updater(s) updates the data.
    /// # Returns
    /// `Ok()` if the latest data can be retrieved.
    /// `Err()` if the data has not been updated since the last call of `recv` or `try_recv` and there is no updater of this channel.
    ///
    /// # Examples
    /// ## Basic usage
    /// ```
    /// use latest_value_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.recv());
    /// ```
    /// ## Receive after updater dropped
    /// ```
    /// use latest_value_channel::channel;
    ///
    /// let (updater, receiver) = channel::<i32>();
    ///
    /// drop(updater);
    /// // The updater dropped and no data exists on the buffer. So recv() fails.
    /// assert!(receiver.recv().is_err());
    /// ```
    /// ## Receive after update and updater dropped
    /// ```
    /// use latest_value_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// updater.update(1).unwrap();
    /// drop(updater);
    ///
    /// // The updater dropped. but the updated data remains, so recv() succeeds.
    /// assert_eq!(Ok(1), receiver.recv());
    ///
    /// // The updater dropped and no data exists on the buffer. So recv() fails.
    /// assert!(receiver.recv().is_err());
    /// ```
    pub fn recv(&self) -> Result<T, RecvError> {
        use std::time::Duration;

        loop {
            match self.try_recv() {
                Ok(data) => break Ok(data),
                Err(TryRecvError::Disconnected) => break Err(RecvError),
                Err(TryRecvError::Empty) => {
                    // NOTE: It may be desirable if this timeout duration can be specified by users.
                    let dur = Duration::from_millis(1);
                    let guard = self.buffer.lock().unwrap();

                    // Wait a notification from the updater(s) without consuming CPU time.
                    // Use wait_timeout() instead of wait().
                    // wait() blocks the current thread forever if the updater(s) dropped after the previous try_recv() is called.
                    //
                    // Condvar's wait methods may cause spurious wakeup.
                    // take() will return None under spurious wakeup because the updater(s) does not update the data yet.
                    // Thus, spurious wakeup does not corrupt channel's integrity.
                    if let Some(data) = self.condvar.wait_timeout(guard, dur).unwrap().0.take() {
                        // The updater(s) sended notification during wait_timeout()
                        break Ok(data);
                    }
                    // Arrival here means no updater sends notification, the updater(s) dropped or the updater(s) sended it before wait_timeout() is called.
                    // Under the first case the data may become available later.
                    // Under the second case, this loop will end in the next loop because try_recv() will return TryRecvError::Disconnected.
                    // Under the third case the latest data will be captured by try_recv() in the next loop.

                    // In summary, this loop never become infinite loop!
                }
            }
        }
    }

    /// Attempts to receive the latest data of this channel. This method does not block the current thread.
    ///
    /// After successful reception, nothing can be retrieved unless the updater(s) updates the data.
    /// *See also the examples of [`Receiver::recv`](struct.Receiver.html#method.recv) to understand behaviors when the updater(s) has disconnected.*
    /// # Returns
    /// `Ok()` if the latest data can be retrieved.
    /// `Err()` if
    /// - The data has not been updated since the last call of `recv` or `try_recv`.
    /// - There is no updater of this channel.
    ///
    /// # Examples
    /// ```
    /// use latest_value_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    /// // Nothing updated yet.
    /// assert!(receiver.try_recv().is_err());
    ///
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.try_recv());
    ///
    /// // Nothing updated yet from the last reception.
    /// assert!(receiver.try_recv().is_err());
    /// ```
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        // lock() fails if another owner of the mutex has panicked during holding it.
        // But the owners (updater) never panic at the time.
        // Therefore, this unwrap() always succeeds.
        match self.buffer.lock().unwrap().take() {
            Some(data) => Ok(data),
            None => {
                let has_updater = self.has_updater();
                if has_updater {
                    Err(TryRecvError::Empty)
                } else {
                    Err(TryRecvError::Disconnected)
                }
            }
        }
    }

    /// Returns `true` if there is at least 1 updater of this channel.
    fn has_updater(&self) -> bool {
        Arc::weak_count(&self.buffer) != 0
    }
}

impl<T> IntoIterator for Receiver<T> {
    type Item = T;
    type IntoIter = RecvIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        RecvIter { receiver: self }
    }
}

/// Represents an iterator that continuously calls [`Receiver::recv`](struct.Receiver.html#method.recv) of a channel.
/// *See also [`Receiver::into_iter`](struct.Receiver.html#method.into_iter).*
///
/// This iterator returns the latest data of the channel as `Some(data)`.
/// After the sender(s) of the channel has disconnected, this iterator returns `None`.
/// # Examples
/// ```
/// use latest_value_channel::channel;
/// use std::thread::{sleep, spawn};
/// use std::time::Duration;
///
/// let (updater, receiver) = channel();
///
/// let join_handle = spawn(move || {
///     for i in 0..10 {
///         updater.update(i).unwrap();
///         sleep(Duration::from_millis(10));
///     }
/// });
///
/// let receptions = receiver.into_iter().collect::<Vec<_>>();
/// join_handle.join().unwrap();
///
/// assert_eq!((0..10).collect::<Vec<_>>(), receptions);
/// ```
#[derive(Debug)]
pub struct RecvIter<T> {
    receiver: Receiver<T>,
}

impl<T> Iterator for RecvIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        self.receiver.recv().ok()
    }
}

/// An error returned from the `recv` function on a `Receiver`.
///
/// This error occurs if `Receiver` has become unable to receive data anymore due to the updater(s)' disconnection.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RecvError;

impl Display for RecvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "This receiver has become unable to receive data anymore due to the updater(s)' disconnection.")
    }
}

impl Error for RecvError {}

/// An error returned from the `try_recv` function on a `Receiver`.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum TryRecvError {
    /// This error occurs if `Receiver` has become unable to receive data anymore due to the updater(s)' disconnection.
    Disconnected,
    /// This channel is currently empty, but the updater(s) have not yet disconnected, so data may yet become available.
    Empty,
}

impl Display for TryRecvError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let msg = match self {
            TryRecvError::Disconnected => "This receiver has become unable to receive data anymore due to the updater(s)' disconnection.",
            TryRecvError::Empty => "This channel is currently empty, but the updater(s) have not disconnected yet, so data may become available.",
        };
        write!(f, "{}", msg)
    }
}

impl Error for TryRecvError {}

/// The updating-half of the single data channel.
///
/// Data of this the channel can be updated by `update`.
/// `update` will overwrite the existing data of the channel.
///
/// The `Updater` can be cloned to update to the same channel multiple times.
#[derive(Debug)]
pub struct Updater<T> {
    /// Destination to the buffer of the recevier.
    dest: Weak<Mutex<Option<T>>>,
    /// Sends notifications that the data is updated to the receiver.
    condvar: Weak<Condvar>,
}

impl<T> Updater<T> {
    /// Updates the data of this channel.
    /// Previous data will be overwritten if it exists.
    /// # Returns
    /// `Ok()` if the data is updated.
    /// `Err()` if there is no receiver of this channel.
    ///
    /// # Examples
    /// ## Basic usage
    /// ```
    /// use latest_value_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// updater.update(1).unwrap();
    /// assert_eq!(Ok(1), receiver.recv());
    /// ```
    /// ## `update` after `Receiver` dropped
    /// ```
    /// use latest_value_channel::channel;
    ///
    /// let (updater, receiver) = channel();
    ///
    /// // update() fails because there is no receiver of this channel.
    /// drop(receiver);
    /// assert!(updater.update(1).is_err());
    /// ```
    pub fn update(&self, data: T) -> Result<(), UpdateError<T>> {
        if let (Some(dest), Some(condvar)) = (self.dest.upgrade(), self.condvar.upgrade()) {
            // lock() fails if another owner of the mutex has panicked during holding it.
            // But the owners (receiver and other updaters) never panic at the time.
            // Therefore, this unwrap() always succeeds.
            dest.lock().unwrap().replace(data);
            // Notify the receiver that a new data is stored.
            condvar.notify_one();
            Ok(())
        } else {
            Err(UpdateError(data))
        }
    }
}

impl<T> Clone for Updater<T> {
    fn clone(&self) -> Self {
        Self {
            dest: self.dest.clone(),
            condvar: self.condvar.clone(),
        }
    }
}

/// An error returned from the `update` function on a `Updater`.
///
/// This error occurs if the receiver has become disconnected, so the data could not be updated.
/// The data is returned back to the callee in this case.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct UpdateError<T>(T);

impl<T> Display for UpdateError<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "The receiver has disconnected, so the data could not be updated."
        )
    }
}

impl<T: Debug> Error for UpdateError<T> {}

/// Creates a new channel, returning the updater/receiver halves.
///
/// The `Updater` can be cloned to update to the same channel multiple times, but only one `Receiver` is supported.
/// # Examples
/// ```
/// use latest_value_channel::channel;
///
/// let (updater, receiver) = channel();
///
/// updater.update(1).unwrap();
/// assert_eq!(Ok(1), receiver.recv());
/// ```
pub fn channel<T>() -> (Updater<T>, Receiver<T>) {
    let buffer = Arc::from(Mutex::from(None));
    let dest = Arc::downgrade(&buffer);
    let condvar = Arc::from(Condvar::new());
    let weak_condvar = Arc::downgrade(&condvar);
    let receiver = Receiver { buffer, condvar };
    let updater = Updater {
        dest,
        condvar: weak_condvar,
    };

    (updater, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_recv() {
        let (updater, receiver) = channel();

        updater.update(1).unwrap();
        assert_eq!(Ok(1), receiver.recv());

        updater.update(2).unwrap();
        assert_eq!(Ok(2), receiver.recv());
    }

    #[test]
    fn test_multiple_update_recv() {
        let (updater, receiver) = channel();
        // Update
        updater.update(1).unwrap();
        updater.update(2).unwrap();

        // Only the latest data will be received
        assert_eq!(Ok(2), receiver.recv());
    }

    #[test]
    fn test_multiple_updaters_recv() {
        let (updater, receiver) = channel();
        let updater2 = updater.clone();
        // Update
        updater.update(1).unwrap();
        assert_eq!(Ok(1), receiver.recv());
        updater2.update(2).unwrap();
        assert_eq!(Ok(2), receiver.recv());

        // Only the latest data will be received
        updater.update(10).unwrap();
        updater2.update(20).unwrap();
        assert_eq!(Ok(20), receiver.recv());

        // Channel is valid since at least 1 updater exists.
        drop(updater);
        updater2.update(200).unwrap();
        assert_eq!(Ok(200), receiver.recv());
    }

    #[test]
    fn test_update_recv_multithread() {
        let (updater, receiver) = channel();

        // Update on another thread
        std::thread::spawn(move || {
            updater.update(1).unwrap();
            updater.update(2).unwrap();

            // Updater dropped here, but there is data in buffer of the channel.
            // The data can be retrieved by the receiver.
        })
        .join()
        .unwrap();

        // Only the latest data will be received
        assert_eq!(Ok(2), receiver.recv());
    }

    #[test]
    fn test_recv_from_remaining_data_disconnected_updater() {
        let (updater, receiver) = channel();
        updater.update(1).unwrap();
        drop(updater);

        // The updater dropped.
        // but the updated data remains, so recv() succeeds.
        assert_eq!(Ok(1), receiver.recv());
    }

    #[test]
    fn test_recv_no_data_disconnected_updater() {
        let (updater, receiver) = channel::<i32>();
        drop(updater);

        // The updater dropped and no data exists on the buffer.
        // So recv() fails.
        assert_eq!(Err(RecvError), receiver.recv());
    }

    #[test]
    fn test_try_recv() {
        let (updater, receiver) = channel();

        // Nothing updated yet
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
        // Update/receive
        updater.update(1).unwrap();
        assert_eq!(Ok(1), receiver.try_recv());

        // Nothing updated from the last reception
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_multiple_updaters() {
        let (updater, receiver) = channel();
        let updater2 = updater.clone();

        // Only the latest data will be received
        updater.update(1).unwrap();
        updater2.update(2).unwrap();
        assert_eq!(Ok(2), receiver.try_recv());

        // Nothing updated from the last reception
        assert_eq!(Err(TryRecvError::Empty), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_from_remaining_data_disconnected_updater() {
        let (updater, receiver) = channel();
        updater.update(1).unwrap();
        drop(updater);

        // The updater dropped.
        // but the updated data remains, so try_recv() succeeds.
        assert_eq!(Ok(1), receiver.try_recv());

        // try_recv fails because nothing updated from the last reception and the updater dropped.
        assert_eq!(Err(TryRecvError::Disconnected), receiver.try_recv());
    }

    #[test]
    fn test_try_recv_no_updater() {
        let (updater, receiver) = channel::<i32>();
        drop(updater);

        assert_eq!(Err(TryRecvError::Disconnected), receiver.try_recv());
    }

    #[test]
    fn test_update_no_receiver() {
        let (updater, receiver) = channel();
        drop(receiver);

        assert_eq!(Err(UpdateError(1)), updater.update(1));
    }
}

#[cfg(test)]
mod tests_iter {
    use super::*;

    #[test]
    fn test_iter() {
        let (updater, receiver) = channel();

        let join_handle = std::thread::spawn(move || {
            for i in 0..10 {
                updater.update(i).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });

        let receptions = receiver.into_iter().collect::<Vec<_>>();
        join_handle.join().unwrap();

        assert_eq!((0..10).collect::<Vec<_>>(), receptions);
    }

    #[test]
    fn test_iter_multiple_update() {
        let (updater, receiver) = channel();

        updater.update(1).unwrap();
        updater.update(2).unwrap();

        assert_eq!(Some(2), receiver.into_iter().next());
    }

    #[test]
    fn test_iter_multiple_updater() {
        let (updater, receiver) = channel();
        let updater2 = updater.clone();

        updater.update(1).unwrap();
        updater2.update(2).unwrap();

        assert_eq!(Some(2), receiver.into_iter().next());
    }

    #[test]
    fn test_iter_from_remaining_data_disconnected_updater() {
        let (updater, receiver) = channel();
        updater.update(1).unwrap();
        drop(updater);

        assert_eq!(vec![1], receiver.into_iter().collect::<Vec<_>>());
    }

    #[test]
    fn test_iter_empty() {
        let (updater, receiver) = channel::<i32>();
        drop(updater);

        assert!(receiver.into_iter().next().is_none());
    }
}
