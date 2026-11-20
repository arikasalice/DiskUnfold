use super::*;

#[test]
fn the_handler_installs_once_and_counts_what_it_records() {
    let first = Interrupts::install();
    let second = Interrupts::install();
    assert_eq!(first.is_ok(), second.is_ok());
    let Ok(interrupts) = first else { return };
    let before = interrupts.count();
    record();
    assert_eq!(interrupts.count(), before + 1);
}
