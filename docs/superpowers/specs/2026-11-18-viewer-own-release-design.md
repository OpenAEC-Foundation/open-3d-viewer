# The viewer as a program of its own, released beside the suite

**Status:** design of 2026-11-18; nothing is built yet.

## Intent

Someone who wants to look at a model looks for a 3D viewer. "BIM suite" tells that person the
program is large and for specialists, and they stay away. So the viewer is positioned as the
simple way in, and the suite as its extended version:

- **Open 3D Viewer Studio** opens a model, turns it, measures it and cuts it. Nothing else shows.
- **Open BIM Suite** is the same program with the other products beside the viewer.

Both come from this code base and from one release run, with one version number. Each has its
own installer and its own release: the viewer in `OpenAEC-Foundation/open-3d-viewer-studio`, the
suite here.

## Goals

- A program that shows the Viewer only, under the name Open 3D Viewer Studio everywhere a user
  meets it.
- The viewer takes back the identity the app was first released with (program name, install
  folder, package name, update id), so a copy installed today becomes the viewer at its next
  update. The suite gets an identity of its own and installs beside it.
- One release run builds, signs, tests and publishes both; a tag gives two releases.

## Non-goals

- A viewer compiled without the code of the other products (see Approaches). It can follow
  without another change of names.
- The browser version, the Android app and the snap: they stay as they are.
- Entries for the two programs in the OpenAEC Installer's catalogue. It lists neither today.
- Installers for the Modeller, Validator and Pointcloud on their own.
- A settings folder per program: the two keep sharing one.

## What would make this a failure

- "BIM Suite" where a viewer user sees it: window title, Start menu, installer, the program's
  properties and Task Manager, About, the release title.
- An update that leaves someone without a working program, or with two shortcuts that start the
  same thing.
- A suite release that goes out while the viewer release silently does not.

## Today

- The release ships one program, the Studio edition: every product, with the switch at the top
  right. It carries the viewer's old identity for everything a machine keys on and the suite's
  name for what a user reads:
  - Windows installs in `%LOCALAPPDATA%\Programs\Open 3D Viewer Studio` under the uninstall key
    `Open 3D Viewer Studio`, with a shortcut and display name "Open BIM Suite"
    (`packaging/windows/installer.nsi`).
  - macOS: `Open BIM Suite.app` whose `Info.plist` says `Open 3D Viewer Studio` and
    `org.openaec.Open3DViewer` (`packaging/macos/Info.plist.in`).
  - Linux: package `open-3d-viewer`, desktop file `org.openaec.Open3DViewer.desktop` with
    `Name=Open BIM Suite` (`packaging/linux/`).
  - Release files are named `open-3d-viewer_<version>_...`.
- In the code, `Edition::Studio` takes the Viewer's name and id: `("Open BIM Suite",
  "open-3d-viewer")` (`src/product.rs`). A viewer-only edition would get the same two.
- No viewer-only program exists. `--no-default-features --features viewer` builds the Studio
  with fewer products: the switch still shows Viewer, Struct, GIS and Docs.
- `build.rs` writes one Windows version resource for every program of the package: ProductName
  and FileDescription "Open BIM Suite".
- The settings folder is `OpenAEC/Open BIM Suite` for every program (`src/remote_link.rs`); an
  older `OpenAEC/Open 3D Viewer Studio` folder was moved to that name once.
- `open-3d-viewer-mcp` starts the program named `open-3d-viewer` next to it, and the Vertex
  reader's library is looked for in `../lib/open-3d-viewer/` (`src/bin/open-3d-viewer-mcp.rs`,
  `src/loaders/vertex.rs`).
- The last step of the release copies the whole suite release to `open-3d-viewer-studio`. It
  has copied nothing yet, because its secret `VIEWER_STUDIO_RELEASE_TOKEN` is not set; that
  repository holds 2026.47.1, put there by hand.

## The two programs

| | Viewer | Suite |
|---|---|---|
| Name a user reads | Open 3D Viewer Studio | Open BIM Suite |
| Shows | the Viewer, no switch | every product of the build, with the switch |
| Program | `open-3d-viewer` | `open-bim-suite` |
| MCP server beside it | `open-3d-viewer-mcp` | `open-bim-suite-mcp` |
| Edition in the code | `Edition::Only(Product::Viewer)` | `Edition::Studio` |
| Update id | `open-3d-viewer` | `open-bim-suite` |
| Windows folder and uninstall key | `Open 3D Viewer Studio` | `Open BIM Suite` |
| Linux package | `open-3d-viewer` | `open-bim-suite` |
| Linux desktop, MIME and icon id | `org.openaec.Open3DViewer` | `org.openaec.OpenBIMSuite` |
| macOS bundle and its id | `Open 3D Viewer Studio.app`, `org.openaec.Open3DViewer` | `Open BIM Suite.app`, `org.openaec.OpenBIMSuite` |
| Release files | `open-3d-viewer_<version>_...` | `open-bim-suite_<version>_...` |
| Release | `open-3d-viewer-studio`, "Open 3D Viewer Studio <version>" | this repository, "Open BIM Suite <version>" |

The viewer's column is what releases up to 2026.46.0 had. The suite's column is new.

## Approaches

1. **Two programs from one build (chosen).** One `cargo build` per platform, with the default
   features, links both programs; packaging and signing run once per program. The release run
   grows by minutes, and both programs are the build the tests ran on. The viewer holds the
   other products' code, which cannot be reached from its interface.
2. **A slim viewer build.** The viewer is compiled again without the other products' features.
   The download is smaller, but every platform compiles twice (the Windows and macOS jobs take
   26 and 24 minutes now) and the slim build is not the one the tests ran on.
3. **One program, two installers.** The installer leaves a mark that decides what the program
   shows. Cheapest, but a program that is copied or renamed changes product.

The positioning is about what people see, which the first settles in full at the lowest cost
and risk. The second can replace it later behind the same names. The third is dropped.

## Design

### The programs

- `open-3d-viewer` (`src/main.rs`) starts the desktop app as `Edition::Only(Product::Viewer)`.
- A new program `open-bim-suite` starts it as `Edition::Studio`.
- The browser version must show what it shows today, so it is built from the Studio. How (the
  program trunk builds, the names in `index.html` and `web/sw.js`) is left to the builder.
- Names and ids in `src/product.rs`:
  - `Product::Viewer`: "Open 3D Viewer Studio", `open-3d-viewer`.
  - `Edition::Studio`: "Open BIM Suite", `open-bim-suite`, no longer taken from the Viewer.
- A product an edition does not offer cannot be reached in it: not by the switch, a command, a
  shortcut, `--product`, or what the shared preferences remember.
- The same holds for the modes without a window that belong to another product: `--check` (the
  Validator), `--docs` and `--gis`. The viewer refuses them with one line that names
  `open-bim-suite`, and exits with an error code. `--check` was documented for scripts under the
  name `open-3d-viewer` in 2026.47.2: the notes of the first split release say that it moved.
- Each program's Windows version resource carries its own ProductName and FileDescription; the
  MCP servers carry their program's name with "MCP server".

### The name everywhere

Whatever shows the program's name to a user, or sends it off the machine as the program's name,
follows the edition. The window title, status bar and About already do. The rest today says
"Open BIM Suite" whatever the edition:

- the welcome text, error texts that name the program, the MCP server's title and instructions;
- what exports write as author or application (IFC, IFCX, reports, the PDF producer, the Docs
  register, the library);
- the User-Agent of the app's web requests (one of them still says `Open3DViewerStudio/`);
- the feedback form's app id, which is the edition's id.

A file saved by the viewer says Open 3D Viewer Studio made it.

### Settings, the MCP server and the Vertex library

- Both programs keep reading and writing `OpenAEC/Open BIM Suite`. A second move of that folder
  is not worth what it would cost people who run both, and nobody sees its name in the app.
- The MCP server is one program built once, packaged under two names. It starts the program
  whose name is its own without `-mcp`; `O3V_VIEWER` still overrides that. `--print-config`
  names the server after the program too.
- The remote link stays one file (`remote.json`): the MCP server talks to the program started
  last, as with two viewers today.
- The Vertex library is looked for in the lib folder of the program's own package
  (`../lib/open-3d-viewer/` or `../lib/open-bim-suite/`), after the places it looks in now.

### Packaging

Every package is made once per program, from one description of the programs (the table above)
that the workflows and the packaging files read. No second copy of the names lives in a
workflow.

- **Windows:** two installers and two zips. Each installs its program, its MCP server,
  `vertex.dll` and the icon into its own folder under its own uninstall key, with a Start menu
  shortcut under its own name.
- **macOS:** two bundles and two disk images, each signed, notarized and stapled.
- **Linux:** `.deb`, `.rpm`, AppImage and `.tar.gz` per program and architecture. The two
  packages share no file path, so both install on one machine.
- Both programs get the same icon. An icon per program can come later.

### Existing installs

A copy installed from 2026.46.1 up to 2026.47.x is the suite under the viewer's identity: its
uninstall entry reads "Open BIM Suite". A copy from 2026.46.0 or before is a viewer: its entry
reads "Open 3D Viewer Studio". Either installer can be the first to meet such a copy.

**The viewer's installer first**

- **Windows:** it goes into that same folder and key. It replaces `open-3d-viewer.exe` and the
  uninstaller, removes the shortcut `Open BIM Suite.lnk` when that points into this folder, and
  makes `Open 3D Viewer Studio.lnk`.
- **Linux:** an upgrade of package `open-3d-viewer` gives the viewer; its menu entry reads Open
  3D Viewer Studio again.
- **macOS:** `Open 3D Viewer Studio.app` is a new name beside an `Open BIM Suite.app` already in
  Applications; the new suite image replaces that one when it is dragged over it.

**The suite's installer first**

Without care this leaves two programs called Open BIM Suite, and the old one's uninstaller,
which is already out, deletes the shortcut `Open BIM Suite.lnk` whoever made it.

- **Windows:** when the suite's installer finds the viewer's uninstall key with the display
  name "Open BIM Suite", that copy is the suite it replaces: it removes that copy's files, key
  and shortcut itself (not through the old uninstaller), then installs. A copy whose display
  name is "Open 3D Viewer Studio" is a viewer and is left alone.
- **Linux:** package `open-bim-suite` replaces a package `open-3d-viewer` older than the first
  split version (Conflicts and Replaces in the `.deb`, Obsoletes in the `.rpm`). A viewer of
  that version or later installs beside it. A viewer of 2026.46.0 or before goes too: the
  package's version cannot tell it from the suite, and the suite holds everything it did.
- **macOS:** nothing to do.

Apart from that one case, neither installer touches the other's folder, key, shortcut or files.

Who used the other products installs the suite once. The notes of both releases say so, each
with a link to the other, for as long as 2026.47.x is recent.

### The release run

- `check`: as now.
- `linux`, `windows`, `macos`: build once, package and sign per program, upload one artifact per
  platform that holds both programs' files.
- `linux-headless`: runs its test on the AppImage of each program, on both architectures. The
  notes of each release give that program's memory at rest.
- `publish`:
  1. sorts the files by program;
  2. writes `SHA256SUMS` per program and signs it and the Linux packages with OpenPGP, per
     program;
  3. attests every package of both;
  4. publishes the suite's release here, as now, with its own notes;
  5. publishes the viewer's release in `open-3d-viewer-studio`, with the secret
     `VIEWER_STUDIO_RELEASE_TOKEN`, the tag on that repository's `main`, the title "Open 3D
     Viewer Studio <version>" and its own notes.
- The viewer's notes name the viewer's files and say that the source is here and that
  `gh attestation verify` takes `--repo OpenAEC-Foundation/open-bim-suite`.
- Both releases are a pre-release when any signature is missing, as now.
- **When the viewer's release cannot be published** (no secret, a token that expired, a network
  error), the suite's release stays and the job fails. A release that is half out must be seen.
  Running the job again publishes what is missing and leaves alone what is there, in either
  repository.
- A dry run builds, signs and tests both and publishes nothing.
- The step that copies the suite's release to `open-3d-viewer-studio` goes.

### Continuous integration

`build.yml` builds both programs natively on every platform it builds now, and runs the headless
test on both on Linux. Which artifacts a pull request keeps is left to the builder.

### Documents

- `README.md` and `README.nl.md` here: the download section names the suite's files and points
  to the viewer's repository for the viewer; the paragraph on releases describes the two
  releases; paths and commands that name the program follow.
- `docs/architecture/products.md`: the table of builds, the bullets on names and ids, and its
  open questions 2 (names) and 4 (installers) as far as this settles them.
- The README of `open-3d-viewer-studio` says today that its installers are the suite's. It is
  edited once, by hand in that repository, right after the first viewer release is out: the
  files there are the viewer. Its text is not kept here and no workflow writes it.

## Left to the builder

- How the one description of the programs is held and read by NSIS, the spec file, the desktop
  and MIME files, `Info.plist` and the workflows.
- How each program gets its own version resource from one package.
- How the app's code learns its edition where it writes the program's name.
- The order of the work and how it is split over pull requests, as long as `main` can make a
  correct release after every merge.
- The wording of the release notes.

## How it is checked

- Unit tests: the names and ids of the two editions; the viewer edition offers the Viewer alone
  and shows no switch; a product outside the edition cannot be started by a command or by
  `--product`; the MCP server's choice of program from its own name.
- A dry run of the release workflow on the branch, before merging: every package of both
  programs is built and signed, the headless test passes on both AppImages, and the Windows job
  installs both installers side by side and finds two folders, two uninstall keys and two
  shortcuts.
- On a Windows machine with 2026.47.x installed, both orders:
  - viewer first: one program called Open 3D Viewer Studio and no shortcut to "Open BIM Suite";
    the suite's installer then adds the suite beside it;
  - suite first: one program called Open BIM Suite, one entry in the installed apps, a shortcut
    that starts it; the viewer's installer then adds the viewer beside it.
- On Linux with the 2026.47.x package installed: installing `open-bim-suite` removes it and
  leaves one menu entry; installing the new `open-3d-viewer` beside it gives two, each under its
  own name.
- The first real release: two releases with the same tag, each with its own checksums that
  verify.

## Before the first release

Releases here hold only the suite's files from then on. What reads them must be ready:

- **The secret** `VIEWER_STUDIO_RELEASE_TOKEN` exists in this repository: a token that may write
  the contents of `open-3d-viewer-studio` alone. Without it the release run fails at its last
  step.
- **The website** (repository `website`) lists the viewer's downloads, release notes and tool
  entry from this repository's releases, by the name `open-3d-viewer` (`scripts/
  generate-downloads.js`, `generate-release-notes.js`, `build-tools-api.js`,
  `build-markdown-mirrors.js`). Those entries read `open-3d-viewer-studio` instead, or the
  viewer's page offers the suite. A page for the suite is the website's own matter.
- **OpenAEC Linux** (the SuperCloud's image) runs the suite. It is built from the AppImage of a
  release here, which is named `open-bim-suite_<version>_<arch>.AppImage` from then on: what
  builds the image looks for that name. The README's section on OpenAEC Linux describes the
  suite's AppImage; the headless test still runs on both.
- **The feedback service** files a report under the app id it is sent and makes an issue of it
  through a mapping from app id to repository. It has one for `open-3d-viewer`; it gets one for
  `open-bim-suite`, to this repository. Without it the suite's reports are kept but cannot be
  made into issues.

## What follows from it, not built here

- The snap is made from the single `.deb` of a release and installs `open-3d-viewer`: it takes
  the viewer's `.deb` from the viewer's repository once that exists.
- `scripts/product_video.py` reads the remote link from a settings folder the app no longer
  uses.
- The Validator's update id, `open-bim-validator`, is also the id of another program in the
  OpenAEC Installer's catalogue.
