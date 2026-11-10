//! The project list (ADR-0009): root subfolders, the test group, registered folders,
//! and the same refusals a card's project gets. Every folder here is a throwaway
//! one under the temp folder: a fake home with O's state folder beside it.

use std::fs;
use std::path::{Path, PathBuf};

use o_core::context::normalize_project;
use o_core::projects::{self, default_root, Kind, Listing};

struct Fixture {
    base: PathBuf,
    /// The fake user home.
    user: PathBuf,
    /// O's state folder, outside the fake home as on Windows (LOCALAPPDATA).
    home: PathBuf,
}

fn fixture(name: &str) -> Fixture {
    let base = std::env::temp_dir().join(format!("o-projects-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    let user = base.join("user");
    let home = base.join("local").join("project_O");
    fs::create_dir_all(&user).unwrap();
    fs::create_dir_all(&home).unwrap();
    Fixture { base, user, home }
}

fn slashes(path: &Path) -> String {
    normalize_project(&path.to_string_lossy())
}

fn names(listing: &Listing, kind: Kind) -> Vec<&str> {
    listing.projects.iter().filter(|p| p.kind == kind).map(|p| p.name.as_str()).collect()
}

#[test]
fn registration_refuses_agent_profiles_and_windows_system_folders() {
    let f = fixture("protected");
    let profile = f.home.parent().unwrap().join("project_O-agents").join("claude").join("home");
    fs::create_dir_all(&profile).unwrap();
    assert!(projects::check(&f.home, &f.user, &profile.to_string_lossy()).unwrap_err().contains("프로필"));
    #[cfg(windows)]
    if let Some(windows) = std::env::var_os("SystemRoot") {
        assert!(projects::check(&f.home, &f.user, &windows.to_string_lossy()).unwrap_err().contains("시스템"));
    }
}

#[test]
fn the_default_root_is_inside_the_home_folder_not_the_state_folder() {
    let f = fixture("default");
    let root = default_root(&f.user);
    assert_eq!(root, f.user.join(".project_o").join("project"));
    assert!(!root.starts_with(&f.home));

    let listing = projects::list(&f.home, &f.user).unwrap();
    assert_eq!(listing.root, slashes(&root));
    assert!(listing.default_root);
    assert!(!listing.root_exists);
    assert!(listing.projects.is_empty());

    // Creating the root makes only that folder, inside the home and away from O's state.
    let listing = projects::create_root(&f.home, &f.user).unwrap();
    assert!(listing.root_exists && root.is_dir());
    assert!(!f.home.join(".project_o").exists());
}

#[test]
fn root_subfolders_are_projects_and_the_test_folder_is_a_group() {
    let f = fixture("root");
    let root = default_root(&f.user);
    for dir in ["blog", "Shop", ".hidden", "test/o-audit", "test/probe", "test/.cache"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    fs::write(root.join("notes.txt"), "not a project").unwrap();
    fs::write(root.join("test").join("readme.md"), "not a project").unwrap();

    let listing = projects::list(&f.home, &f.user).unwrap();
    assert_eq!(names(&listing, Kind::Root), ["blog", "Shop"]);
    assert_eq!(names(&listing, Kind::Test), ["o-audit", "probe"]);
    let audit = listing.projects.iter().find(|p| p.name == "o-audit").unwrap();
    assert_eq!(audit.path, slashes(&root.join("test").join("o-audit")));
    assert!(!audit.path.contains('\\'));
}

#[test]
fn registering_normalizes_dedupes_and_never_touches_the_folder() {
    let f = fixture("add");
    let outside = f.base.join("work").join("app");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "keep").unwrap();

    let spelled = format!("{}\\", outside.to_string_lossy().replace('/', "\\"));
    let listing = projects::add(&f.home, &f.user, &spelled).unwrap();
    let registered: Vec<_> = listing.projects.iter().filter(|p| p.kind == Kind::Registered).collect();
    assert_eq!(registered.len(), 1);
    assert_eq!(registered[0].path, slashes(&outside));
    assert_eq!(registered[0].name, "app");

    // The same folder again, in another spelling, is refused rather than listed twice.
    let again = if cfg!(windows) { slashes(&outside).to_uppercase() } else { format!("{}/", slashes(&outside)) };
    assert!(projects::add(&f.home, &f.user, &again).unwrap_err().contains("이미 목록"));

    // The list lives in O's state folder and survives a fresh read.
    assert!(f.home.join(projects::FILE).is_file());
    assert_eq!(names(&projects::list(&f.home, &f.user).unwrap(), Kind::Registered), ["app"]);

    // Taking it off forgets the path only.
    let listing = projects::remove(&f.home, &f.user, &spelled).unwrap();
    assert!(names(&listing, Kind::Registered).is_empty());
    assert_eq!(fs::read_to_string(outside.join("keep.txt")).unwrap(), "keep");
    assert!(projects::remove(&f.home, &f.user, &spelled).unwrap_err().contains("등록된 폴더가 아닙니다"));
}

/// The app, the preview window and `o` share one list: changes made at the same moment all stay.
#[test]
fn changes_made_at_the_same_moment_are_all_kept() {
    let f = fixture("together");
    let folders: Vec<PathBuf> = (0..12).map(|i| f.base.join("elsewhere").join(format!("p{i:02}"))).collect();
    for folder in &folders { fs::create_dir_all(folder).unwrap(); }
    let other = f.base.join("another-root");
    std::thread::scope(|s| {
        for folder in &folders {
            s.spawn(|| projects::add(&f.home, &f.user, &folder.to_string_lossy()).unwrap());
        }
        s.spawn(|| projects::set_root(&f.home, &f.user, Some(&other.to_string_lossy())).unwrap());
    });
    let listing = projects::list(&f.home, &f.user).unwrap();
    let mut registered = names(&listing, Kind::Registered);
    registered.sort_unstable();
    assert_eq!(registered, (0..12).map(|i| format!("p{i:02}")).collect::<Vec<_>>());
    assert_eq!(listing.root, slashes(&other));
    // Taking some off while others go on loses neither.
    std::thread::scope(|s| {
        for folder in &folders[..6] {
            s.spawn(|| projects::remove(&f.home, &f.user, &folder.to_string_lossy()).unwrap());
        }
        s.spawn(|| projects::set_root(&f.home, &f.user, None).unwrap());
    });
    let listing = projects::list(&f.home, &f.user).unwrap();
    assert_eq!(names(&listing, Kind::Registered).len(), 6);
    assert_eq!(listing.root, slashes(&default_root(&f.user)));
    fs::remove_dir_all(&f.base).unwrap();
}

#[test]
fn registering_refuses_what_a_card_project_may_not_be() {
    let f = fixture("refuse");
    let reason = |path: &str| projects::add(&f.home, &f.user, path).unwrap_err();

    // The home folder itself, O's state folder, a folder around it, a missing one, a relative one.
    assert!(reason(&f.user.to_string_lossy()).contains("홈 폴더"));
    assert!(reason(&f.home.to_string_lossy()).contains("상태 폴더"));
    assert!(reason(&f.base.join("local").to_string_lossy()).contains("상태 폴더"));
    assert!(reason(&f.base.join("gone").to_string_lossy()).contains("없습니다"));
    assert!(reason("relative/folder").contains("절대 경로"));

    // The root and its test folder are containers, not projects.
    let root = default_root(&f.user);
    fs::create_dir_all(root.join("test")).unwrap();
    assert!(reason(&root.to_string_lossy()).contains("루트"));
    assert!(reason(&root.join("test").to_string_lossy()).contains("루트"));

    // A folder the root lists already, and one inside the home folder, is fine to check.
    fs::create_dir_all(root.join("blog")).unwrap();
    assert!(reason(&root.join("blog").to_string_lossy()).contains("이미 목록"));
    assert!(projects::check(&f.home, &f.user, &root.join("blog").to_string_lossy()).is_ok());
    assert!(!f.home.join(projects::FILE).exists(), "nothing refused was saved");
}

#[test]
fn root_projects_are_not_unregistered_and_a_missing_registered_folder_stays_listed() {
    let f = fixture("missing");
    let root = default_root(&f.user);
    fs::create_dir_all(root.join("blog")).unwrap();
    assert!(projects::remove(&f.home, &f.user, &root.join("blog").to_string_lossy()).unwrap_err().contains("루트 안의 폴더"));

    let gone = f.base.join("gone-later");
    fs::create_dir_all(&gone).unwrap();
    projects::add(&f.home, &f.user, &gone.to_string_lossy()).unwrap();
    fs::remove_dir(&gone).unwrap();
    let listing = projects::list(&f.home, &f.user).unwrap();
    let entry = listing.projects.iter().find(|p| p.kind == Kind::Registered).unwrap();
    assert!(entry.missing);
    assert!(projects::remove(&f.home, &f.user, &gone.to_string_lossy()).is_ok());
}

#[test]
fn another_root_is_checked_and_the_default_comes_back() {
    let f = fixture("setroot");
    assert!(projects::set_root(&f.home, &f.user, Some(&f.user.to_string_lossy())).unwrap_err().contains("홈 폴더"));
    assert!(projects::set_root(&f.home, &f.user, Some(&f.home.join("x").to_string_lossy())).unwrap_err().contains("상태 폴더"));

    let other = f.base.join("elsewhere");
    let listing = projects::set_root(&f.home, &f.user, Some(&other.to_string_lossy())).unwrap();
    assert_eq!(listing.root, slashes(&other));
    assert!(!listing.default_root && !listing.root_exists);
    assert!(!other.exists(), "choosing a root does not make it");

    let listing = projects::set_root(&f.home, &f.user, None).unwrap();
    assert!(listing.default_root);
    assert_eq!(listing.root, slashes(&default_root(&f.user)));
}

#[test]
fn a_new_project_is_an_empty_folder_in_the_root() {
    let f = fixture("new");
    assert!(projects::new_project(&f.home, &f.user, "blog").unwrap_err().contains("루트 폴더가 없습니다"));
    projects::create_root(&f.home, &f.user).unwrap();

    let (path, listing) = projects::new_project(&f.home, &f.user, " 새 블로그 ").unwrap();
    assert_eq!(path, format!("{}/새 블로그", slashes(&default_root(&f.user))));
    assert!(Path::new(&path).is_dir());
    assert_eq!(names(&listing, Kind::Root), ["새 블로그"]);

    for bad in ["", "a/b", "..", ".hidden", "trailing.", "con", "COM1.txt", "test", "x:y"] {
        assert!(projects::new_project(&f.home, &f.user, bad).is_err(), "{bad:?} should be refused");
    }
    assert!(projects::new_project(&f.home, &f.user, "새 블로그").unwrap_err().contains("이미 있습니다"));
}

#[test]
fn an_unreadable_list_is_an_error_and_is_left_as_it_is() {
    let f = fixture("corrupt");
    fs::write(f.home.join(projects::FILE), "{ not json").unwrap();
    let outside = f.base.join("work");
    fs::create_dir_all(&outside).unwrap();
    assert!(projects::list(&f.home, &f.user).is_err());
    assert!(projects::add(&f.home, &f.user, &outside.to_string_lossy()).is_err());
    assert_eq!(fs::read_to_string(f.home.join(projects::FILE)).unwrap(), "{ not json");
}
