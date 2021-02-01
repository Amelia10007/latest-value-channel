# latest-value-channel [![crates.io](https://img.shields.io/crates/v/latest-value-channel.svg)](https://crates.io/crates/latest-value-channel) [![Build Status](https://travis-ci.org/Amelia10007/latest-value-channel.svg?branch=master)](https://travis-ci.org/Amelia10007/latest-value-channel)
Multi-producer, single-consumer ***SINGLE*** data communication primitives.

This crate provides a latest-message style channel, where the updater(s) can update the latest data that the receiver owns it.

Unlike the `std::sync::mpsc::channel`, by using `channel` of this crate, the stored data will be overwritten.
Once the `receiver` receives the data of its channel, `receiver` can retrieve nothing unless the updater(s) updates the data.

These property are useful when a thread is interested in the latest result of another continually working thread and wants to use the latest data only once.
For example, this crate may be useful if you want to use measured values of sensors (such as camera, force-torque sensor, etc.).

# Examples
## Basic usage
```rust
use latest_value_channel::channel;

let (updater, receiver) = channel();

std::thread::spawn(move || {
    updater.update(1).unwrap(); // Update to 1
    updater.update(2).unwrap(); // Update to 2
})
.join().unwrap();

assert_eq!(Ok(2), receiver.recv()); // Receive the latest value
```
## Disconnected channel's behavior
```rust
use latest_value_channel::channel;

let (updater, receiver) = channel();

updater.update(1).unwrap();
drop(updater);

// The updater dropped. But the updated data remains, so recv() succeeds.
assert_eq!(Ok(1), receiver.recv());

// The updater dropped and no data exists on the buffer. So recv() fails.
assert!(receiver.recv().is_err());
```

# Similar functionality module and crate
- [std::sync::mpsc::channel](https://doc.rust-lang.org/std/sync/mpsc/index.html)
- [single_value_channel](https://crates.io/crates/single_value_channel)
- [update_channel](https://crates.io/crates/update_channel)
- [latest](https://crates.io/crates/latest)
