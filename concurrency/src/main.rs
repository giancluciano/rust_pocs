use std::thread;
use std::sync::mpsc;


fn main() {
    // simple thread 
    let simple_thread = thread::spawn(move || {
        println!("hello from thread");
    });
    let _result = simple_thread.join();

    // configure thread before spawning it
    let thread_1 = thread::Builder::new().name("thread 1".to_string()).spawn(move || {
        println!("hello from thread 1");
    });

    let _res = thread_1.expect("reason").join();

    // using channels to pass data
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let val = String::from("hello");
        tx.send(val).unwrap();
    });

    let received = rx.recv().unwrap();
    println!("Got {received}");
}
