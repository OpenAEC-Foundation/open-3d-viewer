//! Docs of Open BIM Suite: the documents of a project.
//!
//! A project is a folder that a sync tool or a network drive keeps the same on everyone's machine:
//! Dropbox, SharePoint (through OneDrive), Synology Drive, or any shared folder. No server and no
//! account: who can reach the folder takes part. The team's own folders and files stay as they
//! are; what is said about a document is kept beside them, in [`REGISTER_FOLDER`].
//!
//! Headless: no GUI and no clock. The app passes the time, the person's name and new ids.
//!
//! - [`folder`]: what is in the project folder, and what the sync tool is doing to each file
//!   (only online, a conflict copy).
//! - [`service`]: which sync tool keeps the folder.
//! - [`git`]: a folder that is a Git checkout (GitHub among them), and the versions Git keeps.
//! - [`register`]: what is said about each document (title, status, revisions, who has it checked
//!   out, the elements it is about), one small IFCX file per document, so two people working on
//!   different documents never write the same file; and the copies kept of recorded revisions.
//! - [`share`]: where a file is on the website of the service that shares the folder, and a mail
//!   that hands it on.
//!
//! Three rules hold everywhere (`docs/superpowers/specs/2026-11-17-docs-design.md`):
//!
//! - Nothing is written unless a person does something. Whether a file changed since its last
//!   recorded revision is worked out when the folder is read.
//! - No file is opened to list it: a file that is only online would be downloaded.
//! - No project file is moved, renamed or deleted. A file is only written over when a person puts
//!   an earlier version back.

pub mod folder;
pub mod git;
pub mod register;
pub mod service;
pub mod share;

pub use folder::{File, Kind, Listing};
pub use register::{Document, ElementLink, Out, Register, Revision, State, Status};
pub use service::{Known, Service, Sync};

/// The suite's own folder inside a project folder. Its name starts with an underscore, not a dot:
/// it sorts first, it is in sight, and no sync tool leaves it out.
pub const SUITE_FOLDER: &str = "_openaec";
/// The register inside a project folder: one IFCX file per document.
pub const REGISTER_FOLDER: &str = "_openaec/docs";
/// The copies of recorded revisions inside a project folder: a folder per document.
pub const VERSIONS_FOLDER: &str = "_openaec/versions";
