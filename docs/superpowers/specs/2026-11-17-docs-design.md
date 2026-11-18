# Docs: the documents of a project, in a folder the team already shares

**Status:** design of 2026-11-17; the first slice is built with it.

## Intent

A project is more than its models: drawings, PDFs, reports, schedules. A team needs one place that
says which files a project has, which revision of each stands, and what state it is in. Docs is that
place in Open BIM Suite.

It needs no server and no account. Teams already keep their project folder the same on everyone's
machine with a sync tool or a network drive. Docs works on that folder as it is: Dropbox, SharePoint
(through OneDrive), Synology Drive, or any network or local folder.

It works closely with IFCX. What Docs knows about a document is itself IFCX, and a document can
point at elements of a model.

## Goals

- Show the project folder: its folders and files, what kind each file is, when it changed.
- Say per document: a title, a status, the revision that stands, and the revisions recorded before.
- Show what the sync tool is doing to a file: only online, or a conflict copy.
- Open a model in the suite, and any other file in the program the computer opens it with.
- Link a document to elements of the open model.
- Keep working when two people use it at once.

## Non-goals (for now)

- Storing copies of old revisions. The sync tools keep a history of their own; Docs records which
  revision was which (date, size, by whom, a note).
- Rights and approvals per person. Who can reach the folder takes part.
- The browser build. A synced folder is on the desktop only.
- Web links to a file at the sync service.

## Anti-goals

- Docs never writes on its own. A register that every machine updates when it notices a changed
  file makes all of them write the same file at once, and the sync tool answers with conflict
  copies. Docs writes only when a person does something: sets a status, records a revision, links.
- Docs never opens a file to list it. A file that is only online would be downloaded.
- Docs never moves, renames or deletes a project file.

## Approaches

1. **One register file for the project** (a JSON or IFCX file with every document). Simple to
   read, but two people recording at the same time write the same file: a conflict copy every time.
2. **A database beside the folder** (SQLite). Fast, but a database file in a synced folder breaks
   when two machines write it, and it is not an open format.
3. **One small IFCX file per document** (chosen). Two people working on different documents never
   write the same file, as the shared issues of the BIM Validator already do. When two do change
   the same document, the sync tool leaves a second file with the same document id in it; Docs
   reads both as one (the entry changed last stands, the revisions and links of both are kept) and
   tidies up at the next write. The files are IFCX, so any IFCX tool reads them.

## Design

### In the project folder

```
<project folder>/                 kept the same by the sync tool
  …                               the team's own folders and files, untouched
  _openaec/docs/<id>.ifcx         one per document that something was said about
```

A file nothing was said about has no entry: it shows as "not recorded".

### A register entry

An IFCX file in the form the suite writes everywhere (header, no imports, no schemas). Its data:

- The document's node, at the path of its id, with `bsi::ifc::prop::Name` (its title or file name)
  and `openaec::document`: `id`, `file` (path in the project folder, `/` between names), `title`,
  `status` (`work`, `shared`, `published`, `archived`), `revision`, `revisions` (each: `code`,
  `date`, `by`, `note`, `size`, `modified`), `elements`, `changed` and `by`.
- For each linked element that has an IFCX node path: a node at that path with
  `openaec::document::<id>` (`name`, `file`, `status`, `revision`). Layered over the model, the
  element carries its documents, in this suite and in any IFCX tool.

An element link holds the element's lasting key (GlobalId, else UniqueId, else its id), its IFCX
node path when it has one, its name, and the model file it was in.

### Changed since

Whether a file changed since its last recorded revision is worked out when the folder is read,
from the file's size and time against the revision's. Nothing is written for it.

### The sync tool

- Which one: the markers the tools leave at the root of what they sync, and the folders Windows
  reports for OneDrive and SharePoint libraries. Otherwise a network path, or a local folder.
- Only online: the file attributes Windows sets for a placeholder.
- Conflict copy: the names the three tools give one.
- Left out of the list: the tools' own files, temporary and lock files, and `_openaec`.

### In the window

- Docs is a product on the switch at the top of the window, in every desktop build. Its ribbon has
  a tab Docs (the project folder; opening and linking the picked document) and the shared View tab.
- Its page fills the workspace, also when no model is open: the files of the picked folder (name,
  status, revision, changed, size; a document's title and marks under its name). A line of text
  above them says which folder and which sync tool. The folders are the left pane.
- The other products do not show the page. The properties of a selected element list the documents
  about it there, and a click on one goes to Docs.
- No buttons above the list. Choosing the folder, refreshing, opening, the status, recording a
  revision and the links are sections of the right pane, as with the IFCX tab.
- With elements selected and a document picked: "Link to selection". A link in the right pane
  selects its elements in the model.

### Code

- `crates/bim-docs`: headless. `folder` (what is in the folder), `service` (which sync tool),
  `register` (the entries, their IFCX form, merging). No GUI, no clock: the app passes the time,
  the person's name and new ids.
- `src/app/docs.rs`: the page, its right-pane sections, the folder dialog, opening files. Desktop
  only.
- The folder chosen is kept in the settings folder (`docs-folder.txt`), per person.

## Second slice (2026-11-18): files as in a file manager, saving, Git

Docs should feel like working with files: go into a folder, open a model or a point cloud straight
in the suite, save it back, and see its versions. And a project can be shared through GitHub as
well as through a sync tool.

### As a file manager

- The folders of the folder that shows stand above its files, with a way up (`..`). A click goes in.
- A second click on a file soon after the first opens it: what the suite reads (a model, a point
  cloud) in the suite, anything else in the program the computer opens it with.

### Saving

- "Save model in project folder" (the ribbon, and the project folder's section of the right pane).
- A model that was opened from the project folder, in a format the suite writes (IFCX, IFC), is
  saved where it is. Any other model is saved as a new IFCX file in the folder that shows, and
  never over a file that is there.
- After the save Docs reads the folder again and picks the file, ready to record a revision.
- This is a person's act, so the rule that Docs never writes on its own holds.

### Git and GitHub

- A project folder that is (in) a Git checkout is known by its `.git`; where it comes from is
  read from the checkout's own config (`origin`), without any name, password or token in the
  address. From `github.com` it is called GitHub, otherwise Git.
- For the picked file Docs asks the `git` program for the versions it keeps (who, when, the first
  line of why) and how the file stands against the last one: the same, changed, or not in Git yet.
  Only for the picked file, on a thread of its own.
- "Open" on a version writes that version to a file of its own in the temporary folder
  (`<name> @ <hash>.<extension>`) and opens it: beside the open model, so the two can be compared.
- Docs never commits, fetches, pulls or pushes. Sharing the work stays with the person's own Git
  tool, as it stays with the sync tool for the other services.

### Not in this slice

- Committing and pushing from Docs; signing in to GitHub; a repository that is not cloned yet.
- The version history that Dropbox, SharePoint and Synology Drive keep at their own side.
- New folder, rename, move and delete: Docs still changes no project file but the one it saves.

## Left to the builder

Column widths, icons, the wording of marks, how many files a scan takes before it says there are
more, and the order of the right-pane sections.
