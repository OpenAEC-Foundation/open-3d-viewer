# Modeller: the INB library Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** the wall, floor, roof, column and beam types of the INB template are a library a project adds to its own, placed with the Wall, Slab, Column and Beam tools under the names the template gives them.

**Architecture:** the catalogue extracted from the template (`crates/bim-library/data/inb-catalogue.json`, as Open BIM Studio ships it) is compiled into `bim-library` and read by `bim_library::inb::library()`. A type of the library can carry a name of its own (`BuildUp::name`, `LinearType::name`), shown in place of the rule's. The Library tab's "Add INB types" merges the catalogue into the project's library.

**Tech Stack:** Rust, `bim-library`, `bim-core`; cargo with `RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc`, the shared `target/`.

**Spec:** `docs/superpowers/plans/2026-11-14-modeller-roadmap.md`, sub-project 7. Reference: Open BIM Studio `main`, `localisation/nl/types/README.md` and `catalogue.json`.

## Global Constraints

- Nothing is read at run time: the catalogue is compiled in.
- The catalogue's types have ids of their own (`inb.<role>.<name>`); a named type is a type of its own beside an unnamed one of the same layers.
- Modeller only; one branch; every text translated; commits end with `Co-Authored-By: Claude Fable 5.2 <noreply@anthropic.com>`.

## Review Focus

1. A project that takes the catalogue twice gets nothing the second time.
2. A column or beam of a library type is whole without the library: its profile carries the type's name and shape. It carries no material: a linear element has none in the kernel yet.
3. The Column and Beam tools list the library's own types before the steel table.

## Not in this plan

A group of their own for the catalogue's types in the Library tab (they are told by their names). The material of a column or beam on the element. The 844 beam and 824 column types of `INB-Library 0.1.ifc`: the catalogue follows the template, as the reference does.

---

### Task 1: The catalogue as a library

**Files:** Create `crates/bim-library/src/inb.rs`, `crates/bim-library/data/inb-catalogue.json`; modify `crates/bim-library/src/buildup.rs`, `profile.rs`, `library.rs` (`name`, `matching_named`), `src/app/library.rs` (`LibraryMessage::AddInb`), `src/app/placement.rs` (the Column and Beam tools list the library's types), `src/app/modeller.rs` (`tool_shape`), `src/app/modeller/file_types.rs` (a type taken from a file keeps its name), `locales/*.json`.

- [x] Tests: the catalogue gives 39 walls, 31 floors, 2 roofs, 7 columns and 7 beams under their names; a project takes it once; a wall and a column of an INB type are placed and say what they are made of.
- [x] Implement; translations; whole suite; commit; push.
