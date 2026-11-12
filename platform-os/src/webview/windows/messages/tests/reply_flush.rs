use super::*;

#[test]
fn reply_produced_during_post_is_sent_in_same_flush() -> Result<(), Box<dyn std::error::Error>> {
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    owner.queue_test_reply(Reply::ipc("1".into(), "first", None));
    let mut posts = Vec::new();
    assert!(deliver_reply_batch(&owner, |wire| {
        posts.push(wire.to_owned());
        if posts.len() == 1 {
            owner.queue_test_reply(Reply::ipc("2".into(), "second", None));
        }
        Ok::<(), io::Error>(())
    }));
    assert_eq!(posts, ["1\nfirst", "2\nsecond"]);
    Ok(())
}

#[test]
fn reply_produced_during_every_post_stops_at_batch_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let app = app(|request| Some(request.to_owned()), ordered, None);
    let pool = WorkerPool::start(&app)?;
    let owner = pool.controller_owner()?;
    owner.queue_test_reply(Reply::ipc("0".into(), "payload", None));
    let mut posts = 0;
    assert!(deliver_reply_batch(&owner, |_| {
        posts += 1;
        owner.queue_test_reply(Reply::ipc(posts.to_string(), "payload", None));
        Ok::<(), io::Error>(())
    }));
    assert_eq!(posts, QUEUE_LIMIT);
    assert_eq!(owner.reply_count(), 1);
    Ok(())
}
