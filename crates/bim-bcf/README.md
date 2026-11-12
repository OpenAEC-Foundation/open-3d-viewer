# Native BCF 2.1 archives

`BcfArchive::read` imports ZIP bytes; `topics` returns summaries; `set_status`
updates only the XML status attribute; `write` exports the retained files.
Unmodified file payloads, including comments, cameras, attachments and metadata,
are preserved byte for byte. ZIP compression metadata and directory entries are
not preserved. Limits are 50 MiB compressed, 100 MiB expanded, and 2000 entries.

`from_topics` creates validation topics with UUID identifiers, RFC3339 creation
dates and IFC GlobalIds. Viewpoint identifiers are deterministic UUID v5 values.
The example exports topics with and without component selections:

```sh
cargo run -p bim-bcf --example export -- validation.bcfzip
cargo test -p bim-bcf
```

Camera summaries expose the first viewpoint's perspective or orthographic camera.
Component selections and viewpoint/visibility/coloring/clipping counts aggregate
all viewpoints. Visibility, coloring, clipping planes, snapshots and comments are
retained, but this library does not apply them to a renderer. Camera field of view
accepts `(0, 180)` degrees, consistent with the forward-compatibility advice in
the BCF 2.1 schema. Zero and parallel direction/up vectors are rejected.

The importer checks XML syntax, required topic/viewpoint identifiers, references,
archive safety and camera validity; it is **not a complete XSD validator**.
Generated example markup, viewpoints and version XML were independently checked
against the official buildingSMART BCF 2.1 schemas at revision
`92fdcf4bfaa8f0576e7562730f341fc7781e2065`:
[schemas](https://github.com/buildingSMART/BCF-XML/tree/92fdcf4bfaa8f0576e7562730f341fc7781e2065/Schemas).
Development schema verification uses an external XML-schema checker; no such
runtime, Python interpreter or network request is required by the Rust library.
