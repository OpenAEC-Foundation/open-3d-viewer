# Library: a first setup

Date: 2026-11-18. Status: a first setup to look at; the cards show, they do not open yet.

## Intent

A project draws on libraries from several places, and today the suite shows only one of them:
the Library tab of the Modeller's explorer, with the project's own types. Library is a product on
the switch at the top of the window that shows all of them side by side:

- **General**: what every project starts with, in any country and at any company.
- **Country**: the content of a country: its drawing standard and what is usual there, in 2D and
  in 3D. This is the localisation of the suite.
- **Company**: the house style of a company: logo, colours and fonts, the title block and
  templates of its drawings, and its own types.
- **Project**: the library of the open project, as the Modeller's Library tab lists it. A library
  file or a component that is opened shows here too.

## Goals of the first setup

- Show how the product could look: its place on the switch, its ribbon, its left pane and its
  page.
- Show real content wherever the suite holds it, so the page is no mock-up.
- Name what is not there yet in the place where it will live, marked "to come".

## Not in the first setup

- Opening a card: the list of its items, a drawing of the picked one, its properties in the right
  pane.
- Choosing, importing or editing a house style or a country's content.
- A library file or a component opened into the Project source.
- Using a library from the page: adding a type to the project, assigning it to a selection.

## What it shows

The page lists the source's content as cards under **2D** and **3D** (and, for a company, **House
style**). A card has a title, a count and a first look: colour swatches, or the first names with
"+n" for the rest. A card of content the suite does not hold says "to come" and is grey.

| Source | 2D | 3D |
|---|---|---|
| General | hatches (ISO 128); to come: line styles, text and dimension styles, symbols | the starter library of `bim-library`: materials, walls, floors, roofs, ceilings, linear types, systems; the steel table; to come: objects |
| Country | hatches of the country's standard; to come: title blocks, line styles, text and dimension styles, symbols | the Netherlands: the types of the Dutch template (`bim_library::inb`); the steel table where European sections are usual; to come: products of manufacturers |
| Company | to come: title block, sheet templates, line weights | to come: own types |
| Project | hatches (NEN 47, as a project draws them today); to come as in General | the project's library (`App::library`) |

The countries are those whose drawing standard `bim-library` knows: the Netherlands (NEN 47),
Germany (DIN 1356), the United Kingdom (ISO 128 / BS 1192) and the United States (ANSI).

A company's house style shows the suite's own colours and its font as the example, until a house
style can be chosen.

## How it is built

- `Product::Library` in `src/product.rs`: on the switch after Docs, grey while in alpha, with one
  ribbon tab (`RibbonPage::Library`). It is in every build, the browser included.
- `src/app/libraries.rs`: the sources and countries, what each holds (`App::library_sections`,
  headless and tested), the left pane and the page.
- `src/shell.rs`: the page is the workspace while Library is the product shown, with or without
  a model; the left pane lists the sources, and the countries under Country; the ribbon has one
  button per source.
- Nothing stands over the page but its own heading.

## Approaches considered

1. **A product with its own page** (chosen). The libraries are about more than the model that
   is open, and a company's house style or a country's content has no place in a pane beside a
   3D view.
2. **More groups in the Modeller's Library tab.** Smaller, but that tab is a narrow pane for the
   project's own types and would hide the country and company content from the other products.
3. **A dialog (a library manager).** Familiar, but a dialog cannot stay open beside the work and
   has no room for content in 2D and 3D side by side.

## The drawing standard (second step, the Netherlands)

The direction after the first setup: Library is where a library is set up in full, and the first
thing to set up is how drawings are made. For now the Netherlands only.

- **A standard is data**: `bim_library::drawing::Standard`. It holds the kinds of drawing and,
  under them, the settings they pick from: scales, line styles, pens (line widths) and text
  styles. Its hatches are those of the country's hatch standard.
- **A kind of drawing** has a name, the code it has in a drawing's name, a number series, a
  discipline, a view, its scales (the usual one first), the pen of what is cut and of what is
  seen beyond, the line style of what is hidden, a text style and whether what is cut is hatched.
- **Where the Dutch content comes from**: the kinds, views, scales and sheet numbers of the
  drawings of the INB template; kinds the template has no drawing of are marked as from practice
  and their scales are a proposal; line styles from ISO 128-2; line widths and text heights from
  the ISO series. The template's style sheets are not taken over: they are under another licence.
- **Kept in IFCX**, a node per setting (`openaec::drawing::standard`, `::kind`, `::linestyle`,
  `::pen`, `::textstyle`), so a later layer (a company, a project) can change one setting. The
  project's file holds the standard next to its library.
- **Checked**: a change is taken only when every kind's scales, pens and styles exist and every
  width, height and dash is positive.
- **In the app**: the page of Country, the Netherlands lists the kinds and the settings; a click
  picks one and the right pane has its fields (`src/app/standards.rs`).

Not yet: the standard does not steer the drawings the suite makes; settings (a scale, a pen, a
line style) cannot be added or removed, only changed; a standard cannot be saved or opened as a
file of its own; other countries have no standard; a kind has one pen pair for all its scales.

## Open questions

- Where a house style is kept: with the company (a file or folder its people share, as Docs uses
  a shared folder), with the project, or both.
- What a country's content is made of beyond its drawing standard, and who maintains it.
- Whether a library file or a component opens in the Project source or as a source of its own.
- What the right pane shows for a picked item, and whether items are edited here or only in the
  Modeller.

## Next steps

1. The standard steers the drawings: a floor plan, section or sheet takes its scales, pens,
   line styles and text style from its kind.
2. Add and remove settings; pens per scale; dimension styles, symbols and title blocks as
   settings; the hatches as settings that can be changed.
3. Save and open a standard as an IFCX file of its own, and layer a company's over a country's.
4. Open a card of types: its items as a list with a drawing of the picked one (the drawings
   exist in `src/app/library.rs`).
5. A house style that can be chosen: logo, colours, fonts and title block, kept in a file.
6. Library files and components opened into the page.
