pub mod proxy;

/// The hosts a sandboxed command may reach until the owner sets a list of their own.
pub const DEFAULT_ALLOW: &[&str] = &[
    "index.crates.io",
    "static.crates.io",
    "static.rust-lang.org",
    "github.com",
    "codeload.github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
    "pypi.org",
    "files.pythonhosted.org",
    "registry.npmjs.org",
];
