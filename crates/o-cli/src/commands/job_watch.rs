use super::{fail, Command};

pub const WATCH: Command = Command {
    name: "_job-watch",
    usage: "",
    run: |_, args| o_core::run::job_watch::run(args).map_or_else(|e| fail(&e.to_string()), |()| 0),
};
