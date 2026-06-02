use cobble::commands;
use cobble::commands::{DoctorOptions, InspectOptions};

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

        /// Project template: minimal, stdlib, or validation
        #[arg(long, default_value = "stdlib")]
        template: String,
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

        /// Suppress successful build progress and summary output
        #[arg(short, long)]
        quiet: bool,

        /// Create a zip file
        #[arg(long)]
        zip: bool,

        /// Validate generated .mcfunction files after building
        #[arg(long)]
        validate: bool,

        /// Compile and summarize without writing final output
        #[arg(long)]
        dry_run: bool,

        /// Path to commands.json (default data/commands.json is auto-generated if missing)
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

        /// Path to commands.json (default data/commands.json is auto-generated if missing)
        #[arg(long, default_value = "data/commands.json")]
        commands_json: PathBuf,
    },

    /// Check syntax without building
    Check {
        /// Input file or directory to check
        input: Option<PathBuf>,
    },

    /// Report Cobble project and validation environment status
    Doctor {
        /// Project path to inspect (defaults to current directory)
        path: Option<PathBuf>,

        /// Path to commands.json to inspect
        #[arg(long, default_value = "data/commands.json")]
        commands_json: PathBuf,
    },

    /// Inspect Cobble metadata in a generated data pack directory
    Inspect {
        /// Generated data pack directory to inspect
        input: PathBuf,

        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Validate generated .mcfunction files against Minecraft's command tree
    Validate {
        /// Datapack directory to validate (output from build)
        input: PathBuf,

        /// Path to commands.json (default data/commands.json is auto-generated if missing)
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
            template,
        } => commands::init(commands::init::InitOptions {
            name,
            description,
            pack_format,
            template,
        }),
        Commands::Build {
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            quiet,
            zip,
            validate,
            dry_run,
            commands_json,
        } => commands::build(commands::build::BuildOptions {
            input,
            output,
            namespace,
            pack_format,
            description,
            verbose,
            quiet,
            zip,
            validate,
            dry_run,
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
        Commands::Doctor {
            path,
            commands_json,
        } => commands::doctor(DoctorOptions {
            path,
            commands_json,
        }),
        Commands::Inspect { input, json } => commands::inspect(InspectOptions { input, json }),
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
