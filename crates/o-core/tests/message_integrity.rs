use std::fs;

use o_core::run::{store, team::{self, Speaker}};

#[test]
fn damaged_message_history_cannot_be_replayed_or_appended() {
    let home = std::env::temp_dir().join(format!("o-msg-integrity-{}", store::new_run_id(chrono::Utc::now())));
    let dir = home.join("runs/r1");
    fs::create_dir_all(&dir).unwrap();
    let lead = Speaker { run_id: "r1".into(), me: "lead".into(), cards: vec!["a".into()] };
    let card = Speaker { run_id: "r1".into(), me: "a".into(), cards: vec!["a".into()] };
    let message = "{\"t\":\"msg\",\"id\":\"m1\",\"from\":\"lead\",\"to\":\"a\",\"text\":\"question\"}\n";
    for damaged in [b"{\"t\":\"msg.state\"".as_slice(), b"{\"t\":\"msg.state\",\"id\":\"m1\",\"state\":\"acknowledged\"}\n", b"{\"t\":\"msg\",\"text\":\"\xff\"}\n"] {
        let path = dir.join("events.jsonl");
        fs::write(&path, [message.as_bytes(), damaged].concat()).unwrap();
        assert!(team::claim(&home, &card).is_err());
        assert!(team::read(&home, &card, true).is_err());
        assert!(team::send_tracked(&home, &lead, "a", "again", false, Some("test")).is_err());
        assert!(team::transition(&home, &card, "m1", "acknowledged").is_err());
        assert!(!fs::read(&path).unwrap().windows(5).any(|bytes| bytes == b"again"));
    }
    let target = home.canonicalize().unwrap();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    assert!(target.starts_with(&temp) && target != temp && target.file_name().unwrap().to_string_lossy().starts_with("o-msg-integrity-"));
    fs::remove_dir_all(target).unwrap();
}
