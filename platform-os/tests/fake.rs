#![allow(clippy::unwrap_used)]
#![cfg(feature = "fake")]
use du_core_contracts::ports::{
    Clock, ContentReadPort, EnumeratePort, PortError, PortErrorKind, Random,
};
use du_platform_os::fake::{FakeWorld, Fault, FaultPlan, Node, Operation};
fn world() -> FakeWorld {
    FakeWorld::new(
        vec![
            (b"/".to_vec(), Node::Volume),
            (b"/dir".to_vec(), Node::Directory),
            (b"/dir/a".to_vec(), Node::File(b"hello".to_vec())),
        ],
        42,
    )
    .unwrap()
}
#[test]
fn tree_and_deterministic_random() {
    assert!(
        FakeWorld::new(
            vec![
                (b"/".to_vec(), Node::Directory),
                (b"/bad\0name".to_vec(), Node::Placeholder)
            ],
            1
        )
        .is_err()
    );
    let a = world();
    let b = world();
    assert_eq!(a.enumerate(b"/dir").unwrap().len(), 1);
    assert_eq!(a.read(b"/dir/a", 2).unwrap(), b"he");
    let mut x = [0; 16];
    let mut y = [0; 16];
    a.fill(&mut x);
    b.fill(&mut y);
    assert_eq!(x, y);
    assert!(
        FakeWorld::new(
            vec![
                (b"/".to_vec(), Node::Directory),
                (b"/missing/a".to_vec(), Node::Placeholder)
            ],
            1
        )
        .is_err()
    );
}
#[test]
fn faults_match_operation_path_and_count_and_delay_is_virtual() {
    let world = world();
    world.add_fault(FaultPlan {
        path: b"/dir/a".to_vec(),
        operation: Operation::Read,
        remaining: 1,
        fault: Fault::Error(PortError {
            kind: PortErrorKind::PermissionDenied,
            reason_key: "os.denied".into(),
            os_code: None,
        }),
    });
    assert!(world.enumerate(b"/dir").is_ok());
    assert_eq!(
        world.read(b"/dir/a", 10).unwrap_err().kind,
        PortErrorKind::PermissionDenied
    );
    assert_eq!(world.read(b"/dir/a", 10).unwrap(), b"hello");
    world.add_fault(FaultPlan {
        path: b"/dir/a".to_vec(),
        operation: Operation::Read,
        remaining: 1,
        fault: Fault::Delay(30_000),
    });
    world.read(b"/dir/a", 10).unwrap();
    assert_eq!(world.monotonic_millis(), 30_000);
    world.read(b"/dir/a", 10).unwrap();
    assert_eq!(world.monotonic_millis(), 30_000);
}
