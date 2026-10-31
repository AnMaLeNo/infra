use clap::Parser;

fn main() {
    let cli = infra::cli::Cli::parse();
    match infra::cli::run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("erreur : {e}");
            std::process::exit(1);
        }
    }
}
