use super::super::super::contract::{ResponseDelivery, ResponseDeliveryCallback};
use super::*;
use crate::webview::contract::entry;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

fn recv(
    replies: &mpsc::Receiver<Reply>,
    timeout: Duration,
) -> Result<String, mpsc::RecvTimeoutError> {
    replies
        .recv_timeout(timeout)
        .map(|reply| reply.message.clone())
}

#[test]
fn reply_notifies_posted_only_after_webview_accepts_message() {
    let (order, received) = mpsc::channel();
    let callback_order = order.clone();
    let callback: ResponseDeliveryCallback = Box::new(move |outcome| {
        let _ = callback_order.send(format!("callback:{outcome:?}"));
    });
    let reply = Reply::ipc("17".into(), "{\"name\":\"scan.start\"}", Some(callback));
    assert!(
        reply
            .deliver_with(|message| {
                assert_eq!(message, "17\n{\"name\":\"scan.start\"}");
                let _ = order.send("post".into());
                Ok::<(), ()>(())
            })
            .is_ok()
    );
    assert_eq!(
        received.try_iter().collect::<Vec<_>>(),
        ["post", "callback:Posted"]
    );
}

#[test]
fn reply_post_failure_and_window_drop_report_not_posted_once() {
    let (outcomes, received) = mpsc::channel();
    let callback = |outcomes: &mpsc::Sender<ResponseDelivery>| {
        let outcomes = outcomes.clone();
        Box::new(move |outcome| {
            let _ = outcomes.send(outcome);
        }) as ResponseDeliveryCallback
    };

    let failed = Reply::ipc(
        "18".into(),
        "{\"name\":\"scan.start\"}",
        Some(callback(&outcomes)),
    );
    assert_eq!(
        failed.deliver_with(|_| Err("post failed")),
        Err("post failed")
    );
    let pending = Reply::ipc(
        "19".into(),
        "{\"name\":\"scan.start\"}",
        Some(callback(&outcomes)),
    );
    drop(pending);
    assert_eq!(
        received.try_iter().collect::<Vec<_>>(),
        [ResponseDelivery::NotPosted, ResponseDelivery::NotPosted]
    );
}

#[test]
fn a_closed_reply_lane_discards_queued_scan_candidates() {
    let (outcomes, received) = mpsc::channel();
    let callback_outcomes = outcomes.clone();
    let callback: ResponseDeliveryCallback = Box::new(move |outcome| {
        let _ = callback_outcomes.send(outcome);
    });
    let (sender, receiver) = mpsc::sync_channel(1);
    assert!(
        sender
            .send(Reply::ipc(
                "20".into(),
                "{\"name\":\"scan.start\"}",
                Some(callback),
            ))
            .is_ok()
    );
    drop(receiver);
    assert_eq!(
        received.try_iter().collect::<Vec<_>>(),
        [ResponseDelivery::NotPosted]
    );
}

#[test]
fn scan_receipt_without_admission_callback_is_still_posted() {
    let reply = Reply::ipc("21".into(), "scan receipt", None);
    let mut posted = false;
    assert!(
        reply
            .deliver_with(|message| {
                assert_eq!(message, "21\nscan receipt");
                posted = true;
                Ok::<(), ()>(())
            })
            .is_ok()
    );
    assert!(posted);
}

#[test]
fn worker_prepares_notification_and_post_only_delivers_the_outcome()
-> Result<(), Box<dyn std::error::Error>> {
    let (outgoing, replies) = mpsc::sync_channel(1);
    let (prepared, preparations) = mpsc::channel();
    let (notified, notifications) = mpsc::channel();
    let observer: ResponseDeliveryObserver = Arc::new(move |request, response| {
        prepared
            .send((
                std::thread::current().id(),
                request.to_owned(),
                response.to_owned(),
            ))
            .ok()?;
        let notified = notified.clone();
        Some(Box::new(move |outcome| {
            let _ = notified.send((std::thread::current().id(), outcome));
        }))
    });
    let requests = worker(
        "test-webview-delivery",
        Arc::new(|_| Some("receipt".into())),
        Some(observer),
        outgoing,
    )?;
    requests.send(Request {
        id: "22".into(),
        origin: entry(),
        data: "scan.start".into(),
    })?;
    let timeout = Duration::from_secs(5);
    let (worker_thread, request, response) = preparations.recv_timeout(timeout)?;
    let posting_thread = std::thread::current().id();
    assert_ne!(worker_thread, posting_thread);
    assert_eq!(request, "scan.start");
    assert_eq!(response, "receipt");
    let reply = replies.recv_timeout(timeout)?;
    assert!(matches!(
        notifications.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    reply.deliver_with(|message| {
        assert_eq!(message, "22\nreceipt");
        Ok::<(), io::Error>(())
    })?;
    assert_eq!(
        notifications.recv_timeout(timeout)?,
        (posting_thread, ResponseDelivery::Posted)
    );
    Ok(())
}

#[test]
fn a_blocked_list_lane_does_not_queue_scope_review_behind_it()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let deadline = Duration::from_secs(5);
    let app = WebApp {
        asset: |_| None,
        dispatch: Arc::new(move |command| {
            if command == "list" {
                entered.send(()).ok()?;
                gate.lock().ok()?.recv_timeout(deadline).ok()?;
            }
            Some(command.to_owned())
        }),
        route: |command| {
            if command == "list" {
                DispatchLane::Volume
            } else {
                DispatchLane::Ordered
            }
        },
        response_delivery: None,
        icon: &[],
    };
    let (send, replies) = mpsc::sync_channel(2);
    let queues = IpcQueues::start(&app, send)?;
    assert!(!queues.enqueue("https://example.invalid", "bad", "list"));
    assert!(queues.enqueue(&entry(), "1", "list"));
    started.recv_timeout(deadline)?;
    assert!(queues.enqueue(&entry(), "2", "review"));
    assert_eq!(recv(&replies, deadline)?, "2\nreview");
    release.send(())?;
    assert_eq!(recv(&replies, deadline)?, "1\nlist");
    Ok(())
}
#[test]
fn original_ordered_lane_blocks_close_behind_open() -> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release, gate) = mpsc::channel();
    let gate = Mutex::new(gate);
    let deadline = Duration::from_secs(5);
    let app = WebApp {
        asset: |_| None,
        dispatch: Arc::new(move |command| {
            if command == "browse.open" {
                entered.send(()).ok()?;
                gate.lock().ok()?.recv_timeout(deadline).ok()?;
            }
            Some(command.to_owned())
        }),
        // This reproduces the original app-host classification.
        route: |command| {
            if command == "target.list" {
                DispatchLane::Volume
            } else {
                DispatchLane::Ordered
            }
        },
        response_delivery: None,
        icon: &[],
    };
    let (send, replies) = mpsc::sync_channel(4);
    let queues = IpcQueues::start(&app, send)?;
    assert!(queues.enqueue(&entry(), "1", "browse.open"));
    started.recv_timeout(deadline)?;
    assert!(queues.enqueue(&entry(), "2", "query.region.close"));
    assert!(matches!(
        replies.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    release.send(())?;
    assert_eq!(recv(&replies, deadline)?, "1\nbrowse.open");
    assert_eq!(recv(&replies, deadline)?, "2\nquery.region.close");
    Ok(())
}

#[test]
fn query_controls_overtake_blocked_open_and_volume_without_reordering_normal_requests()
-> Result<(), Box<dyn std::error::Error>> {
    use std::sync::atomic::{AtomicBool, Ordering};
    let (entered, started) = mpsc::channel();
    let (release_open, open_gate) = mpsc::channel();
    let (release_volume, volume_gate) = mpsc::channel();
    let open_gate = Mutex::new(open_gate);
    let volume_gate = Mutex::new(volume_gate);
    let closed = Arc::new(AtomicBool::new(false));
    let state = closed.clone();
    let deadline = Duration::from_secs(5);
    let app = WebApp {
        asset: |_| None,
        dispatch: Arc::new(move |command| match command {
            "browse.open" => {
                entered.send("open").ok()?;
                open_gate.lock().ok()?.recv_timeout(deadline).ok()?;
                Some(
                    if state.load(Ordering::Acquire) {
                        "ref.unavailable"
                    } else {
                        "published"
                    }
                    .to_owned(),
                )
            }
            "target.list" => {
                entered.send("volume").ok()?;
                volume_gate.lock().ok()?.recv_timeout(deadline).ok()?;
                Some("volume".into())
            }
            "query.cancel" => Some("ref.unavailable".into()),
            "query.region.close" => {
                state.store(true, Ordering::Release);
                Some("closed".into())
            }
            other => Some(other.to_owned()),
        }),
        route: |command| match command {
            "target.list" => DispatchLane::Volume,
            "query.cancel" | "query.region.close" => DispatchLane::QueryControl,
            _ => DispatchLane::Ordered,
        },
        response_delivery: None,
        icon: &[],
    };
    let (send, replies) = mpsc::sync_channel(16);
    let queues = IpcQueues::start(&app, send)?;
    assert!(queues.enqueue(&entry(), "1", "browse.open"));
    assert!(queues.enqueue(&entry(), "2", "target.list"));
    let mut both = vec![
        started.recv_timeout(deadline)?,
        started.recv_timeout(deadline)?,
    ];
    both.sort();
    assert_eq!(both, ["open", "volume"]);
    assert!(queues.enqueue(&entry(), "3", "normal.first"));
    assert!(queues.enqueue(&entry(), "4", "normal.second"));
    assert!(!queues.enqueue("https://example.invalid", "untrusted", "query.region.close"));
    assert!(queues.enqueue(&entry(), "5", "query.cancel"));
    assert_eq!(recv(&replies, deadline)?, "5\nref.unavailable");
    assert!(!closed.load(Ordering::Acquire));
    assert!(queues.enqueue(&entry(), "6", "query.region.close"));
    assert_eq!(recv(&replies, deadline)?, "6\nclosed");
    release_open.send(())?;
    assert_eq!(recv(&replies, deadline)?, "1\nref.unavailable");
    assert_eq!(recv(&replies, deadline)?, "3\nnormal.first");
    assert_eq!(recv(&replies, deadline)?, "4\nnormal.second");
    release_volume.send(())?;
    assert_eq!(recv(&replies, deadline)?, "2\nvolume");
    Ok(())
}

#[test]
fn job_controls_keep_order_while_review_and_query_control_are_blocked()
-> Result<(), Box<dyn std::error::Error>> {
    let (entered, started) = mpsc::channel();
    let (release_review, review_gate) = mpsc::channel();
    let (release_query, query_gate) = mpsc::channel();
    let review_gate = Mutex::new(review_gate);
    let query_gate = Mutex::new(query_gate);
    let deadline = Duration::from_secs(5);
    let app = WebApp {
        asset: |_| None,
        dispatch: Arc::new(move |command| {
            let gate = match command {
                "scope.review" => Some(&review_gate),
                "query.region.close" => Some(&query_gate),
                _ => None,
            };
            if let Some(gate) = gate {
                entered.send(command.to_owned()).ok()?;
                gate.lock().ok()?.recv_timeout(deadline).ok()?;
            }
            Some(command.to_owned())
        }),
        route: |command| match command {
            "job.pause" | "job.resume" | "job.cancel" => DispatchLane::JobControl,
            "query.region.close" => DispatchLane::QueryControl,
            _ => DispatchLane::Ordered,
        },
        response_delivery: None,
        icon: &[],
    };
    let (send, replies) = mpsc::sync_channel(8);
    let queues = IpcQueues::start(&app, send)?;
    assert!(queues.enqueue(&entry(), "1", "scope.review"));
    assert!(queues.enqueue(&entry(), "2", "query.region.close"));
    let mut both = [
        started.recv_timeout(deadline)?,
        started.recv_timeout(deadline)?,
    ];
    both.sort();
    assert_eq!(both, ["query.region.close", "scope.review"]);

    assert!(!queues.enqueue("https://example.invalid", "foreign", "job.cancel"));
    assert!(queues.enqueue(&entry(), "3", "job.pause"));
    assert!(queues.enqueue(&entry(), "4", "job.resume"));
    assert!(queues.enqueue(&entry(), "5", "job.cancel"));
    for expected in ["3\njob.pause", "4\njob.resume", "5\njob.cancel"] {
        assert_eq!(recv(&replies, deadline)?, expected);
    }
    release_query.send(())?;
    assert_eq!(recv(&replies, deadline)?, "2\nquery.region.close");
    release_review.send(())?;
    assert_eq!(recv(&replies, deadline)?, "1\nscope.review");
    Ok(())
}
