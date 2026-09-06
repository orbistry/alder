//! Optional semantic progress, independent of diagnostics and presentation.

use url::Url;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    FetchingSources,
    DiscoveringInterfaces,
    ValidatingPackages,
    CompilingModules,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    Phase(Phase),
    ModuleStarted { uri: Url },
}

/// Called synchronously, including from the compiler's blocking worker.
/// Callbacks should return promptly. Diagnostics remain in `BuildResult`.
pub trait Reporter: Send + Sync {
    fn report(&self, event: Progress);
}

impl<F: Fn(Progress) + Send + Sync> Reporter for F {
    fn report(&self, event: Progress) {
        self(event);
    }
}

/// Default for embedding and language-server callers.
pub struct Silent;

impl Reporter for Silent {
    fn report(&self, _: Progress) {}
}
