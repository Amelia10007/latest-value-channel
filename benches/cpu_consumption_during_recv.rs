use latest_value_channel::channel;
use std::time::Duration;

fn main() {
    let (upadter, receiver) = channel();

    let join_handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(10));
        upadter.update(42).unwrap();
    });

    println!("Wait for recieve data. Check CPU consumption.");

    let data = receiver.recv().unwrap();
    assert_eq!(42, data);

    join_handle.join().unwrap();
}
