//! The materialized input to analysis: everything the core is allowed to
//! know about one build output tree. Builders decide what to load; target
//! files are represented by metadata only, image files carry their bytes.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextSource {
    /// Paths supplied by Buildroot's post-image hook environment.
    Hook,
    /// Paths discovered from the tree layout.
    Inferred,
    /// No build tree at all: a bare firmware artifact was analyzed.
    Artifact,
}

impl ContextSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextSource::Hook => "hook",
            ContextSource::Inferred => "inferred",
            ContextSource::Artifact => "artifact",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    /// Native filesystem walk (hardlink dedup available).
    Native,
    /// Browser File API walk (no inode information).
    Browser,
}

impl ScanMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            ScanMode::Native => "native",
            ScanMode::Browser => "browser",
        }
    }
}

/// One file under target/.
#[derive(Debug, Clone)]
pub struct TargetEntry {
    /// Path relative to target/, no leading `./`.
    pub path: String,
    pub size: u64,
    pub is_symlink: bool,
    /// False when this path is a hardlink to an inode already counted under
    /// another path; such entries contribute 0 bytes to totals.
    pub charged: bool,
}

/// One file under images/, bytes included when the builder loaded them.
#[derive(Debug, Clone)]
pub struct ImageInput {
    pub name: String,
    pub size: u64,
    pub bytes: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct NamedText {
    pub name: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Identity of the build, normally the output directory basename.
    pub root_name: String,
    /// Where the scan ran from, for the report header.
    pub root_path: String,
    pub context_source: ContextSource,
    pub scan_mode: ScanMode,

    pub target: Vec<TargetEntry>,
    pub images: Vec<ImageInput>,

    /// Contents of .config (BR2_CONFIG).
    pub config: Option<String>,
    /// Absolute paths on the build host that must not reach the report.
    ///
    /// A Buildroot config is full of them -- where the external tree is, which
    /// defconfig was used, which u-boot fragments were applied -- and a report
    /// is committed, attached to a release and published, so a builder's home
    /// directory and username would go with it. The scanner knows what those
    /// prefixes are; the core does not, and cannot ask the filesystem.
    /// Empty when nothing needs hiding, which is the case in a browser.
    pub redact_prefixes: Vec<String>,
    /// Contents of the defconfig `.config` names in BR2_DEFCONFIG, when that
    /// file is still on disk. The authored profile rather than the expansion:
    /// a couple of dozen lines someone chose, against several hundred the
    /// Kconfig machinery derived from them. Absent for an artifact-only scan,
    /// and for a build tree whose source checkout has gone.
    pub defconfig_text: Option<String>,
    /// Files the scanner was asked to record from the target filesystem.
    ///
    /// A rootfs holds configuration nothing in the build system knows about --
    /// which GPIO drives an IR cut filter, how many steps a pan motor takes --
    /// and none of it is derivable from `.config`. Which files those are is a
    /// project's own business, so they are named on the command line rather
    /// than guessed at here: this end stores bytes and has no opinion about
    /// what is in them.
    pub captured: Vec<NamedText>,
    /// Contents of build/packages-file-list.txt.
    pub pfl: Option<String>,
    /// Contents of build/build-time.log.
    pub build_time_log: Option<String>,
    /// Contents of target/etc/modules (autoload list), if present.
    pub etc_modules: Option<String>,
    /// Contents of the target's os-release, if present. A freedesktop file
    /// Buildroot generates, so it says who built this and from what without
    /// anything here having to know the project.
    pub os_release: Option<String>,
    /// Contents of target/lib/modules/<ver>/modules.builtin, if present.
    pub modules_builtin: Option<String>,
    /// Candidate text files that may carry a flash layout (mtdparts=...).
    pub env_texts: Vec<NamedText>,
    /// genimage configuration files found near the build (or passed in).
    pub genimage_texts: Vec<NamedText>,

    /// Files recorded in packages-file-list.txt but absent from target/,
    /// with source sizes recovered from per-package/ where possible.
    /// Computed by builders with filesystem access; empty otherwise.
    pub removed_candidates: Vec<RemovedCandidate>,

    /// Newest modification time among images/ files (unix seconds); native only.
    pub images_mtime: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct RemovedCandidate {
    /// Path relative to target/, no leading `./`.
    pub path: String,
    pub package: String,
    /// Size of the file in per-package/<pkg>/target, when recoverable.
    pub source_bytes: u64,
}

impl Snapshot {
    /// Empty snapshot for tests and incremental builders.
    pub fn empty(name: &str) -> Self {
        Snapshot {
            root_name: name.to_string(),
            root_path: String::new(),
            context_source: ContextSource::Inferred,
            scan_mode: ScanMode::Native,
            target: Vec::new(),
            images: Vec::new(),
            config: None,
            redact_prefixes: Vec::new(),
            captured: Vec::new(),
            defconfig_text: None,
            pfl: None,
            build_time_log: None,
            etc_modules: None,
            os_release: None,
            modules_builtin: None,
            env_texts: Vec::new(),
            genimage_texts: Vec::new(),
            removed_candidates: Vec::new(),
            images_mtime: None,
        }
    }
}
