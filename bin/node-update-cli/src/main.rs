use exe_runners::CliRunner;

fn main() {
    if let Err(e) =
        CliRunner::default().run_command_until_exit(|ctx| node_update_cli::run(ctx.task_executor))
    {
        eprintln!("ERROR: {e:?}");
    }
}
