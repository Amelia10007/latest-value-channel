use latest_value_channel::channel;
use std::time::Instant;

fn bench_channel<T: Clone + Send + 'static>(data: T, update_count: u32) {
    let (updater, receiver) = channel();

    // Update the data on the another thread
    let join_handle = std::thread::spawn(move || {
        let update_start = Instant::now();
        for _ in 0..update_count {
            // CAUTION:
            // This benchmark contains cloning data
            if let Err(_) = updater.update(data.clone()) {}
        }
        let update_end = Instant::now();
        update_end - update_start
    });

    // Recv the latest data until the updater exists
    let recv_start = Instant::now();
    let recv_count = receiver.into_iter().count();
    let recv_end = Instant::now();

    let recv_average = (recv_end - recv_start) / update_count;
    let update_average = join_handle.join().unwrap() / update_count;

    println!("Update/Recv (count): {} / {}", update_count, recv_count);
    println!(
        "Update/Recv (average time in ns): {} / {}",
        update_average.as_nanos(),
        recv_average.as_nanos()
    );
    println!();
}

fn main() {
    let update_count = 1_000_000;
    println!("i32:");
    bench_channel(42_i32, update_count);

    println!("i64:");
    bench_channel(std::i64::MAX, update_count);

    println!("i128:");
    bench_channel(std::i128::MAX, update_count);

    println!("str:");
    bench_channel("Pekopeko~", update_count);

    println!("Vec1000:");
    bench_channel(vec![42; 1000], update_count);

    println!("Array1000:");
    bench_channel([42; 1000], update_count);
}
