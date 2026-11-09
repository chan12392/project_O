pub fn fake_worker_launch() -> std::io::Result<o_core::sys::launch::Launch> {
    let launch = o_core::sys::launch::Launch::new(std::env::current_exe()?);
    #[cfg(windows)]
    {
        let mut launch = launch;
        launch.handle_owner_executable = Some(env!("CARGO_BIN_EXE_o-handle-owner").into());
        Ok(launch)
    }
    #[cfg(not(windows))]
    { Ok(launch) }
}
