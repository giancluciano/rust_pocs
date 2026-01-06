use std::thread;
use std::sync::{mpsc, Mutex, Arc};
use trpl::Html;



async fn page_title(url: &str) -> Option<String> {
    let response = trpl::get(url).await;
    let response_text = response.text().await;
    Html::parse(&response_text)
        .select_first("title")
        .map(|title| title.inner_html())
}


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

    // using mutexes
    let m = Mutex::new(5);
    {
        let mut num = m.lock().unwrap();
        *num = 6;
    }
    println!("m = {m:?}");

    let counter = Arc::new(Mutex::new(6));
    let mut handles = vec![];

    for _ in 0..10 {
        let counter = Arc::clone(&counter);
        let handle = thread::spawn(move || {
            let mut num = counter.lock().unwrap();
            *num += 1;
        });
        handles.push(handle);
    }
    for handle in handles {
        handle.join().unwrap();
    }
    println!("Result: {}", *counter.lock().unwrap());

    // futures and async

    trpl::block_on(async {
        let title = page_title("https://google.com").await.unwrap();
        println!("title: {title}");
    })

}
