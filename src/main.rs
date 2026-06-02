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

        /// Set the pack format version (default: 101.1 for Minecraft Java Edition 26.1.2)
        #[arg(long)]
        pack_format: Option<String>,
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

        /// Override pack format version (default: 101.1 for Minecraft Java Edition 26.1.2)
        #[arg(long)]
        pack_format: Option<String>,

        /// Override pack description
        #[arg(long)]
        description: Option<String>,

        /// Show verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Create a zip file
        #[arg(long)]
        zip: bool,

        /// Validate generated .mcfunction files after building
        #[arg(long)]
        validate: bool,

        /// Path to commands.json (generated from Minecraft server --reports)
        #[arg(long, default_value = "data/commands.json")]
        commands_json: PathBuf,
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

        /// Pack format version (currently requires 101.1)
        #[arg(long)]
        pack_format: Option<String>,

        /// Data pack description
        #[arg(long)]
        description: Option<String>,

        /// Verbose output
        #[arg(short, long)]
        verbose: bool,

        /// Create a zip file
        #[arg(long)]
        zip: bool,

        /// Validate generated .mcfunction files after each successful build
        #[arg(long)]
        validate: bool,

        /// Path to commands.json (generated from Minecraft server --reports)
        #[arg(long, default_value = "data/commands.json")]
        commands_json: PathBuf,
    },

    /// Check syntax without building
    Check {
        /// Input file or directory to check
        input: Option<PathBuf>,
    },

    /// Validate generated .mcfunction files against Minecraft's command tree
    Validate {
        /// Datapack directory to validate (output from build)
        input: PathBuf,

        /// Path to commands.json (generated from Minecraft server --reports)
        #[arg(long, default_value = "data/commands.json")]
        commands_json: PathBuf,
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
            validate,
            commands_json,
        } => commands::build(commands::build::BuildOptions {
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            zip,
            validate,
            commands_json,
        }),
        Commands::Watch {
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            zip,
            validate,
            commands_json,
        } => commands::watch(
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            zip,
            validate,
            commands_json,
        ),
        Commands::Check { input } => commands::check(input),
        Commands::Validate {
            input,
            commands_json,
        } => commands::validate(commands::validate::ValidateOptions {
            input,
            commands_json,
        }),
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
