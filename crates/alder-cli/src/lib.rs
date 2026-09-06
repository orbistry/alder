/// CLI argument parsing and dispatch.
pub mod cli;
/// Subcommand implementations.
pub mod cmd;
/// GitHub release downloading and archive extraction.
mod download;
/// Compiler version proxy logic.
pub mod proxy;
/// Terminal presentation and optional compiler progress adapter.
pub mod reporting;

pub use cli::Cli;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const BANNER: &str = r#"
      ___           ___           ___           ___
     /\  \         /\  \         /\  \         /\__\
    /::\  \       /::\  \       /::\  \       /:/  /
   /:/\:\  \     /:/\:\  \     /:/\ \  \     /:/__/
  /:/  \:\  \   /::\~\:\  \   _\:\~\ \  \   /::\  \ ___
 /:/__/ \:\__\ /:/\:\ \:\__\ /\ \:\ \ \__\ /:/\:\  /\__\
 \:\  \  \/__/ \/__\:\/:/  / \:\ \:\ \/__/ \/__\:\/:/  /
  \:\  \            \::/  /   \:\ \:\__\        \::/  /
   \:\  \           /:/  /     \:\/:/  /        /:/  /
    \:\__\         /:/  /       \::/  /        /:/  /
     \/__/         \/__/         \/__/         \/__/

 The Alder programming language.

 repo: https://github.com/orbistry/alder
 docs: https://github.com/orbistry/alder
 chat: https://discord.gg/3qQGrKT3eE"#;
