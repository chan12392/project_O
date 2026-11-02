use o_core::run::mode::{take_flags, Efficiency, RunMode};

#[test]
fn efficiency_changes_all_three_behaviors_and_rejects_bad_values() {
    let args = |items: &[&str]| items.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let (on, rest) = take_flags(&args(&["plan.json"])).unwrap();
    assert_eq!(on, RunMode::of(Efficiency::On));
    assert!(on.clean && on.context && on.lower_effort);
    assert_eq!(rest, ["plan.json"]);
    let (off, _) = take_flags(&args(&["--efficiency", "off"])).unwrap();
    assert!(!off.clean && !off.context && !off.lower_effort);
    assert!(take_flags(&args(&["--efficiency", "invalid"])).is_err());
    assert!(take_flags(&args(&["--efficiency", "on", "--efficiency", "off"])).is_err());
}
