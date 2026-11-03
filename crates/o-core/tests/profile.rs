use o_core::run::mode::{take_flags, Profile, RunMode};

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn profile_flag_picks_the_profile_and_keeps_the_context_on() {
    let (default, rest) = take_flags(&args(&["plan.json"])).unwrap();
    assert_eq!(default, RunMode { profile: Profile::ProjectO, context: true, yolo: false });
    assert_eq!(rest, ["plan.json"]);

    let (own, rest) = take_flags(&args(&["--profile", "own", "plan.json"])).unwrap();
    assert_eq!(own, RunMode { profile: Profile::Own, context: true, yolo: false });
    assert!(!own.clean());
    assert_eq!(rest, ["plan.json"]);
    assert_eq!(take_flags(&args(&["--profile=project-o"])).unwrap().0.profile, Profile::ProjectO);

    // `--clean` stays the same as `--profile project-o`; `--no-context` is the only way to drop the context.
    assert_eq!(take_flags(&args(&["--clean"])).unwrap().0, RunMode::of(Profile::ProjectO));
    assert_eq!(take_flags(&args(&["--profile", "project-o", "--clean"])).unwrap().0, RunMode::of(Profile::ProjectO));
    let (no_context, _) = take_flags(&args(&["--profile", "own", "--no-context"])).unwrap();
    assert_eq!(no_context, RunMode { profile: Profile::Own, context: false, yolo: false });
}

#[test]
fn bad_or_conflicting_profiles_and_the_old_switch_are_refused() {
    assert!(take_flags(&args(&["--profile", "clean"])).is_err());
    assert!(take_flags(&args(&["--profile"])).is_err());
    assert!(take_flags(&args(&["--profile", "own", "--clean"])).is_err());
    assert!(take_flags(&args(&["--profile", "own", "--profile", "project-o"])).is_err());
    // The unreleased `--efficiency` switch is gone: it is left for the command to refuse.
    assert_eq!(take_flags(&args(&["--efficiency", "on"])).unwrap().1, ["--efficiency", "on"]);
}

#[test]
fn profile_names_round_trip() {
    for profile in [Profile::ProjectO, Profile::Own] {
        assert_eq!(Profile::parse(profile.as_str()), Some(profile));
        assert_eq!(serde_json::to_value(profile).unwrap(), profile.as_str());
    }
    assert_eq!(Profile::ProjectO.label(), "project_O 프로필");
    assert_eq!(Profile::Own.label(), "내 설정");
}
