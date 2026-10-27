mod cli;

fn main() {
    ovrcr_runtime::server::startup_diagnostic("main_entry");
    cli::main();
}
