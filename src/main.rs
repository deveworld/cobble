use cobble::commands;

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "cobble")]
#[command(about = "Cobble - Minecraft Data Pack Transpiler", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new Cobble project
    Init {
        /// Project name (defaults to current directory name)
        #[arg(long)]
        name: Option<String>,

        /// Set the project description
        #[arg(long)]
        description: Option<String>,

        /// Set the pack format version (default: 88 for Minecraft 1.21.9+)
        #[arg(long)]
        pack_format: Option<u32>,
    },

    /// Build the data pack
    Build {
        /// Input file or directory (defaults to src/ if cobble.toml exists)
        input: Option<PathBuf>,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Override the namespace
        #[arg(long)]
        namespace: Option<String>,

        /// Override pack format version
        #[arg(long)]
        pack_format: Option<u32>,

        /// Override pack description
        #[arg(long)]
        description: Option<String>,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Create a zip file
        #[arg(long)]
        zip: bool,
    },

    /// Watch for changes and rebuild automatically
    Watch {
        /// Input file or directory to watch
        input: Option<PathBuf>,

        /// Output directory
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Data pack namespace
        #[arg(long)]
        namespace: Option<String>,

        /// Pack format version
        #[arg(long)]
        pack_format: Option<u32>,

        /// Data pack description
        #[arg(long)]
        description: Option<String>,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Create a zip file
        #[arg(long)]
        zip: bool,
    },

    /// Check syntax without building
    Check {
        /// Input file or directory to check
        input: Option<PathBuf>,
    },
}

fn main() {
    let cli = Cli::parse();

    let result = match cli.command {
        Commands::Init {
            name,
            description,
            pack_format,
        } => commands::init(commands::init::InitOptions {
            name,
            description,
            pack_format,
        }),
        Commands::Build {
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            zip,
        } => commands::build(commands::build::BuildOptions {
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            zip,
        }),
        Commands::Watch {
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            zip,
        } => commands::watch(
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            zip,
        ),
        Commands::Check { input } => commands::check(input),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
