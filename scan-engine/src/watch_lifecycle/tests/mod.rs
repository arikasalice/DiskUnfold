#![allow(clippy::panic)]
use super::*;
use crate::watch_coalescer::{ChangeKind, Config, GapReason, Signal};

fn definition(id: u8) -> Definition {
    Definition {
        watch_ref: [id; 16],
        scope_revision: [8; 16],
        target: vec![vec![id]],
        entry_path: vec![id],
        volume_path: vec![vec![id]],
        nested_mounts: vec![],
        volume: vec![9],
        baseline_end: Some(1),
    }
}
fn capability() -> Capability {
    Capability {
        notification: Support::Supported,
        loss: Support::Supported,
        interruption: Support::Supported,
        path: Support::Supported,
        resource_limit: Support::Supported,
    }
}
fn registry() -> Registry {
    match Registry::new(4, 32, Config::default()) {
        Ok(v) => v,
        Err(e) => panic!("setup: {e:?}"),
    }
}
fn token(actions: &[Action]) -> u64 {
    match actions.iter().find_map(|a| {
        if let Action::Register { token, .. } = a {
            Some(*token)
        } else {
            None
        }
    }) {
        Some(token) => token,
        None => panic!("register command missing"),
    }
}
fn signal(at: u64) -> Signal {
    Signal {
        path: Some(vec![b"folder".to_vec(), b"file".to_vec()]),
        directory_subtree: false,
        kind: ChangeKind::Modify,
        observed_at: at,
    }
}
#[derive(Default)]
struct FakePort {
    removed: Vec<(u64, u64)>,
}
impl FakePort {
    fn apply(&mut self, actions: &[Action]) {
        for a in actions {
            if let Action::Remove { token, handle, .. } = a {
                self.removed.push((*token, *handle));
            }
        }
    }
}

mod boundaries;
mod core;

mod capacity;
mod producer;
