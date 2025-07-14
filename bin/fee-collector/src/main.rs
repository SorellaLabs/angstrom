use exe_runners::CliRunner;

fn main() {
    if let Err(e) = CliRunner::default()
        .run_command_until_exit(|ctx| angstrom_fee_collector::run(ctx.task_executor))
    {
        eprintln!("ERROR: {e:?}");
        std::process::exit(1);
    }
}
