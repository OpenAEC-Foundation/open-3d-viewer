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

## Editors (third step)

Library is also where the types themselves are made. Each kind of type gets an editor of its own
in the right pane, with a drawing of what it makes above its fields.

- **Materials**, `src/app/materials.rs`: the Project page starts with them, a chip each with
  its hatch and its colour. The editor has the hatch of a section and the colour of the 3D model
  side by side, the name in the library's four languages, the colour (typed as six hexadecimal
  digits, or a slider per channel; the model follows when a slider is let go) and the hatch,
  picked from the hatch kinds drawn and named after the general library's materials. It says
  what is made of the material; one a type is made of is not removed. The key is not edited:
  layers, types and files refer to it. A material of another library comes into the project
  under the key it had, so the types of that library that are copied later find it.
- **Build-ups** (walls, floors, roofs, ceilings), `src/app/buildups.rs`: the Project page lists
  them a role at a time; the editor has the section and the layers, each a material and a
  thickness, to change, move, add and remove; a name of its own or the rule's; add, duplicate,
  remove. A build-up of another library is edited as a copy in the project, with the materials
  it needs. A change is kept only when the build-up still checks.
- **Profiles** (columns, beams and the other linear types), `src/app/profiles.rs`: the Project
  page lists them a role at a time, each with a look at its profile. The editor is based on IFC:
  a profile is one of IFC's parametric profile definitions, shown by its IFC name and set by its
  attributes under their IFC names, as `bim_library::ProfileShape::ifc` gives them and
  `from_ifc` takes them back. Lengths are typed in millimetres, a slope in degrees. The role
  gives the type its IFC class. Under the types stands the steel table, found by typing or by
  series, every profile a tile with its outline; a click gives it to the picked type or makes a
  type of it. A type of another library is edited as a copy in the project.
- **A profile in the file**: the node of a linear type keeps its profile as IFC has it, in
  `openaec::profile`: the `class` of the definition, `ProfileType`, `ProfileName` for a profile
  of the steel table, and the definition's attributes by their IFC names, lengths in metres and
  a slope in radians. It stands beside `openaec::lineartype`, which older readers take the
  profile from. Reading takes the profile from the IFC form when the node has one (attributes
  IFC leaves optional may be missing), so a later layer or another program changes a profile by
  its IFC attributes; a changed profile is a profile of the steel table only when it is named
  as one and is that one. IFCX has no schema for profiles yet, so the attribute is the suite's.
- **Search** over every library and the drawing standard, with a look at each result; a found
  setting, build-up or linear type opens where it is edited, and a found steel profile becomes
  a beam type of the project.
- **Settings** of the drawing standard can be added and removed (scales, line styles, pens, text
  styles), a removal refused while a kind of drawing uses the setting.

Not yet: an editor for system types; a hatch of one's own (a material picks one of the nine
kinds; their patterns follow the drawing standard); a material's category, description and
properties as IFC has them; elements that were given a build-up or a linear type do not follow
a change of the type (they do take a material's new colour); the general library
and a country's types are not edited in place. An arbitrary profile
(`IfcArbitraryClosedProfileDef`) cannot be made. An element made of a type keeps its own copy
of the profile in the kernel's form, not as IFC has it.

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
4. An editor for systems. Profiles drawn freely. Hatches of one's own.
5. Elements follow a change of their type, when asked.
6. A house style that can be chosen: logo, colours, fonts and title block, kept in a file.
7. Library files and components opened into the page.
