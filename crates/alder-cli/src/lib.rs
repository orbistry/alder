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

pub const BANNER: &str = color_print::cstr!(
    r#"
     ___       __       _______   _______ .______      
    /   \     |  |     |       \ |   ____||   _  \     
   /  ^  \    |  |     |  .--.  ||  |__   |  |_)  |    
  /  /_\  \   |  |     |  |  |  ||   __|  |      /     
 /  _____  \  |  `----.|  '--'  ||  |____ |  |\  \----.
/__/     \__\ |_______||_______/ |_______|| _| `._____|

 The <green><bold>Alder</bold></green> programming language.

 <magenta>repo:</magenta> <blue><italic><dim>https://github.com/orbistry/alder</dim></italic></blue>
 <magenta>docs:</magenta> <blue><italic><dim>https://alder-script.dev</dim></italic></blue>
 <magenta>chat:</magenta> <blue><italic><dim>https://discord.gg/3qQGrKT3eE</dim></italic></blue>"#
);
