//! `o sync-mirror <dir>`: fast-forwards a read-only mirror checkout (scheduled task).

use o_core::sys::mirror;

use super::{help, Command};

pub const SYNC_MIRROR: Command = Command {
    name: "sync-mirror",
    usage: "  o sync-mirror <거울 폴더>   거울 폴더(test_2, test_1, main)를 fast-forward (예약 작업용)",
    run: |home, args| {
        let dir = args.first().map(String::as_str);
        if dir.is_none() {
            eprintln!("{}", help());
        }
        mirror::sync_mirror(home, dir)
    },
};
