//! Independent-request parallelism, not intra-file chunk parallelism.
//! Each task owns a complete source buffer and a fresh scanner. No shared parser state.
//! Native builds use a bounded worker pool; wasm32 uses a synchronous fallback.
//! This module currently validates lexing, not Scramjet rewrite equivalence.
use crate::deferred::{DeferredError, DeferredScanner};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request { pub id: u64, pub source: String }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response { pub id: u64, pub result: Result<String, String> }

fn process(request: Request) -> Response {
    let mut scanner = DeferredScanner::new();
    scanner.write(&request.source);
    let result = scanner.finish().map_err(|err| match err {
        DeferredError::Lexical(message) => message,
        DeferredError::Unsupported(offsets) => format!("unsupported syntax at offsets {offsets:?}"),
    });
    Response { id: request.id, result }
}

/// Request isolation means one unsupported source cannot poison other sessions.
/// This sequential implementation is used by wasm32; a browser integration
/// must schedule concurrent instances on separate Web Workers.
pub fn process_sequential(requests: Vec<Request>) -> Vec<Response> {
    requests.into_iter().map(process).collect()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn process_parallel(requests: Vec<Request>, worker_count: usize) -> Vec<Response> {
    use std::sync::{mpsc, Arc, Mutex};
    use std::thread;

    if requests.is_empty() { return Vec::new(); }
    let count = worker_count.max(1).min(requests.len());
    let total = requests.len();
    let (sender, receiver) = mpsc::sync_channel::<(usize, Request)>(count * 2);
    let (result_sender, result_receiver) = mpsc::channel::<(usize, Response)>();
    let shared = Arc::new(Mutex::new(receiver));
    let mut handles = Vec::with_capacity(count);
    for _ in 0..count {
        let queue = Arc::clone(&shared);
        let results = result_sender.clone();
        handles.push(thread::spawn(move || {
            loop {
                let item = match queue.lock() {
                    Ok(guard) => guard.recv(),
                    Err(_) => break,
                };
                match item {
                    Ok((index, request)) => {
                        if results.send((index, process(request))).is_err() { break; }
                    }
                    Err(_) => break,
                }
            }
        }));
    }
    drop(result_sender);
    for (index, request) in requests.into_iter().enumerate() {
        if sender.send((index, request)).is_err() { break; }
    }
    drop(sender);
    let mut output: Vec<Option<Response>> = vec![None; total];
    for (index, response) in result_receiver {
        output[index] = Some(response);
    }
    for handle in handles { handle.join().expect("worker panicked"); }
    output.into_iter().map(|value| value.expect("missing worker result")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_requests_keep_order_and_fail_individually() {
        let input = vec![
            Request { id: 8, source: "window.location;".into() },
            Request { id: 2, source: "const r = /abc/;".into() },
            Request { id: 4, source: "const x = 'ok';".into() },
        ];
        let expected = process_sequential(input.clone());
        assert_eq!(expected[0].id, 8);
        assert!(expected[1].result.is_err());
        assert!(expected[2].result.is_ok());
        #[cfg(not(target_arch = "wasm32"))]
        for workers in [1, 2, 3, 8] {
            assert_eq!(process_parallel(input.clone(), workers), expected);
        }
    }
}
