use cobble::commands;
use cobble::commands::{
    CheckOptions, CleanOptions, DoctorOptions, FmtOptions, InspectOptions, LinkOptions,
    MigrateOptions,
};

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

        /// Project template: minimal, stdlib, validation, resource-heavy, game-mechanic, web-demo, or plugin-diagnostics
        #[arg(long, default_value = "stdlib")]
        template: String,

        /// List available project templates and exit
        #[arg(long)]
        list_templates: bool,
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

        /// Enable experimental resource-pack asset output
        #[arg(long)]
        experimental_resource_pack: bool,

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

        /// Enable experimental resource-pack asset output
        #[arg(long)]
        experimental_resource_pack: bool,

        /// Build to the configured linked data pack target
        #[arg(long)]
        link: bool,

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

        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,

        /// Include experimental editor symbol metadata in JSON output
        #[arg(long)]
        symbols: bool,

        /// Enable the experimental diagnostics-only plugin host skeleton
        #[arg(long)]
        experimental_plugins: bool,

        /// Include the experimental Python compatibility report
        #[arg(long)]
        experimental_python_compat: bool,
    },

    /// Format Cobble source files
    Fmt {
        /// Input file or directory to format
        input: Option<PathBuf>,

        /// Check formatting without writing changes
        #[arg(long)]
        check: bool,

        /// Print formatting differences without writing changes
        #[arg(long)]
        diff: bool,
    },

    /// Report Cobble project and validation environment status
    Doctor {
        /// Project path to inspect (defaults to current directory)
        path: Option<PathBuf>,

        /// Path to commands.json to inspect
        #[arg(long, default_value = "data/commands.json")]
        commands_json: PathBuf,

        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },

    /// Report an experimental Cobble project migration plan
    Migrate {
        /// Project path to inspect (defaults to current directory)
        path: Option<PathBuf>,

        /// Cobble version to migrate from
        #[arg(long, default_value = "0.8")]
        from: String,

        /// Cobble version to migrate to
        #[arg(long, default_value = "0.9")]
        to: String,

        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,

        /// Permit migration rewrites when supported by an action
        #[arg(long)]
        apply: bool,
    },

    /// Remove Cobble-generated project output after safety checks
    Clean {
        /// Project path to read cobble.toml from
        path: Option<PathBuf>,

        /// Output directory to clean
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Show what would be removed without deleting files
        #[arg(long)]
        dry_run: bool,

        /// Clean the configured linked data pack output
        #[arg(long)]
        linked: bool,

        /// Confirm destructive linked cleanup
        #[arg(long)]
        yes: bool,
    },

    /// Configure or inspect a linked Minecraft datapacks target
    Link {
        /// Project path to read cobble.toml from
        path: Option<PathBuf>,

        /// Direct path to a Minecraft datapacks directory
        #[arg(long)]
        datapacks: Option<PathBuf>,

        /// Path to a Minecraft world directory containing datapacks/
        #[arg(long)]
        world: Option<PathBuf>,

        /// Path to a .minecraft directory; uses saves/<pack-name>/datapacks
        #[arg(long)]
        minecraft: Option<PathBuf>,

        /// Directory name for the linked data pack (default: project namespace)
        #[arg(long)]
        pack_name: Option<String>,

        /// Show what would be configured without writing link state
        #[arg(long)]
        dry_run: bool,

        /// Remove saved link state without deleting the linked data pack
        #[arg(long)]
        clear: bool,

        /// Report saved link state and marker status
        #[arg(long)]
        status: bool,
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
            list_templates,
        } => commands::init(commands::init::InitOptions {
            name,
            description,
            pack_format,
            template,
            list_templates,
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
            experimental_resource_pack,
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
            experimental_resource_pack,
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
            experimental_resource_pack,
            link,
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
            experimental_resource_pack,
            link,
            validate,
            commands_json,
        ),
        Commands::Check {
            input,
            json,
            symbols,
            experimental_plugins,
            experimental_python_compat,
        } => commands::check(CheckOptions {
            input,
            json,
            symbols,
            experimental_plugins,
            experimental_python_compat,
        }),
        Commands::Fmt { input, check, diff } => {
            commands::format_sources(FmtOptions { input, check, diff })
        }
        Commands::Doctor {
            path,
            commands_json,
            json,
        } => commands::doctor(DoctorOptions {
            path,
            commands_json,
            json,
        }),
        Commands::Migrate {
            path,
            from,
            to,
            json,
            apply,
        } => commands::migrate(MigrateOptions {
            path,
            from,
            to,
            json,
            apply,
        }),
        Commands::Clean {
            path,
            output,
            dry_run,
            linked,
            yes,
        } => commands::clean(CleanOptions {
            path,
            output,
            dry_run,
            linked,
            yes,
        }),
        Commands::Link {
            path,
            datapacks,
            world,
            minecraft,
            pack_name,
            dry_run,
            clear,
            status,
        } => commands::link(LinkOptions {
            project_path: path,
            datapacks,
            world,
            minecraft,
            pack_name,
            dry_run,
            clear,
            status,
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
