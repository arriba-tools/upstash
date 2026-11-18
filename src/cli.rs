use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "upstash-arriba",
    about = "Upstash CLI that stores notes in a Vector index and recalls them by meaning and keyword",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Store notes in an Upstash Vector index and search them
    ///
    /// The index is the one the environment variables UPSTASH_VECTOR_REST_URL and
    /// UPSTASH_VECTOR_REST_TOKEN name, as the index's Connect section in the Upstash Console gives
    /// them. A namespace of the index holds one project's notes.
    #[command(subcommand)]
    Vector(VectorCommand),
    /// Replace this binary with the one from the latest release
    Upgrade,
}

#[derive(Subcommand)]
pub enum VectorCommand {
    /// Store a note in a namespace, where recall finds it by meaning or by its words, and print
    /// its ID
    Remember(RememberArgs),
    /// Search a namespace's notes by meaning and by keyword together, best match first
    Recall(RecallArgs),
    /// Delete notes by ID
    ///
    /// Exits 1 when the index deleted fewer notes than the IDs given, since the rest named no note.
    Forget(ForgetArgs),
}

#[derive(Args)]
#[command(group(ArgGroup::new("note_source").required(true).args(["file", "text"])))]
pub struct RememberArgs {
    /// Your name
    pub name: String,

    /// Namespace that keeps the note
    pub namespace: String,

    /// UTF-8 file holding the note
    #[arg(long, value_name = "PATH")]
    pub file: Option<PathBuf>,

    /// The note itself
    #[arg(long)]
    pub text: Option<String>,

    /// ID of the stored note this one corrects. The new note takes its ID, so recall no longer finds
    /// the old text
    #[arg(long, value_name = "ID")]
    pub replace: Option<String>,
}

#[derive(Args)]
pub struct RecallArgs {
    /// Namespace whose notes to search
    pub namespace: String,

    /// What to look for, in words, which may start with a dash, as a flag's name does
    #[arg(allow_hyphen_values = true)]
    pub query: String,

    /// How many notes
    #[arg(long, value_name = "N", default_value_t = 5,
          value_parser = clap::value_parser!(u64).range(1..=1000))]
    pub count: u64,

    /// What to match the query by. hybrid ranks by meaning and keyword together, with scores that
    /// only order one query's notes. dense matches by meaning alone, with a score from 0 to 1 that
    /// compares between queries. sparse matches by keyword alone
    #[arg(long, value_enum, default_value_t = Mode::Hybrid)]
    pub mode: Mode,
}

/// The parts of a hybrid index a recall searches.
#[derive(Clone, Copy, Debug, PartialEq, ValueEnum)]
pub enum Mode {
    Hybrid,
    Dense,
    Sparse,
}

#[derive(Args)]
pub struct ForgetArgs {
    /// Namespace that keeps the notes
    pub namespace: String,

    /// IDs of the notes to delete
    #[arg(required = true)]
    pub ids: Vec<String>,
}
