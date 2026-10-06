pub fn launch_xbox_app(hidden: bool) {
    #[cfg(windows)]
    {
        let mut command = std::process::Command::new("cmd");
        if hidden {
            command.args(["/C", "start", "/min", "xbox:"]);
        } else {
            command.args(["/C", "start", "xbox:"]);
        }
        if let Err(err) = command.spawn() {
            tracing::error!(error = %err, hidden, "failed to launch the Xbox app");
        }
    }

    #[cfg(not(windows))]
    {
        let _ = hidden;
        tracing::warn!("the Xbox app can only be launched on Windows");
    }
}
