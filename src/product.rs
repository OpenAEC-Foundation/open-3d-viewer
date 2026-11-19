//! The products built from this code base: the Viewer, IFCX, the Modeller, the BIM Validator,
//! Pointcloud, Structural Analysis, Docs and Library (see `docs/architecture/products.md`).
//!
//! They share one `App` and one window. The switch at the top right of the window moves between
//! them; each shows its own ribbon, panels and start screen over the same open project.

use openaec_ui::design::Look;
use openaec_ui::iced::{self, Element};
use openaec_ui::Language;
use serde::{Deserialize, Serialize};

/// A product: what the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Product {
    /// Open any model, look at it, measure it, cut it.
    #[default]
    Viewer,
    /// Make a BIM model from the base elements of `bim-core`.
    Modeller,
    /// Check models together and agree on issues. Its name was the Collaborator: preferences
    /// saved then still read.
    #[serde(alias = "collaborator")]
    Validator,
    /// Open point clouds and make meshes of them.
    Pointcloud,
    /// Look at structural analysis models: their loads and their results.
    Analysis,
    /// The documents of the project folder: their status, their revisions and the elements they
    /// are about.
    Docs,
    /// Where the project lies and what lies around it: the place, the parcels and buildings,
    /// the terrain.
    Gis,
    /// The libraries a project draws on: the general one, a country's content, a company's house
    /// style and the project's own.
    Library,
    /// The project as its IFCX file has it: its nodes, entries and relations, whatever product
    /// made or opened it.
    Ifcx,
}

impl Product {
    pub const ALL: [Product; 9] = [Product::Viewer, Product::Ifcx, Product::Validator, Product::Pointcloud, Product::Analysis, Product::Modeller, Product::Docs, Product::Gis, Product::Library];

    /// Its key on the command line, in the preferences and in a web address (`?product=`).
    pub fn key(self) -> &'static str {
        match self {
            Product::Viewer => "viewer",
            Product::Modeller => "modeller",
            Product::Validator => "validator",
            Product::Pointcloud => "pointcloud",
            Product::Analysis => "analysis",
            Product::Docs => "docs",
            Product::Gis => "gis",
            Product::Library => "library",
            Product::Ifcx => "ifcx",
        }
    }

    /// The product of a key, in any case; `modeler` too.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Some(Product::Viewer),
            "modeller" | "modeler" => Some(Product::Modeller),
            // its name before it was the Validator, as preferences and links may still say
            "validator" | "bim-validator" | "collaborator" | "bim-collaborator" => Some(Product::Validator),
            "pointcloud" | "point-cloud" => Some(Product::Pointcloud),
            "analysis" | "structural-analysis" => Some(Product::Analysis),
            "docs" | "documents" => Some(Product::Docs),
            "gis" | "gic" => Some(Product::Gis),
            "library" | "libraries" => Some(Product::Library),
            "ifcx" | "ifc" => Some(Product::Ifcx),
            _ => None,
        }
    }

    /// Its name on the switch: short, so the row of products fits beside the title; the same in
    /// every language.
    pub fn label(self) -> &'static str {
        match self {
            Product::Viewer => "View",
            Product::Modeller => "Modeller",
            Product::Validator => "Validator",
            Product::Pointcloud => "PointCloud",
            Product::Analysis => "Struct",
            Product::Docs => "Docs",
            Product::Gis => "GIC",
            Product::Library => "Library",
            Product::Ifcx => "IFCX",
        }
    }

    /// Its name in full, in the list of commands: a product name, the same in every language.
    pub fn full_name(self) -> &'static str {
        match self {
            Product::Viewer => "Viewer",
            Product::Modeller => "Modeller",
            Product::Validator => "BIM Validator",
            Product::Pointcloud => "Pointcloud",
            Product::Analysis => "Structural Analysis",
            Product::Docs => "Docs",
            Product::Gis => "GIS Infra Civil",
            Product::Library => "Library",
            Product::Ifcx => "IFCX",
        }
    }

    /// How far along it is, where it is not ready to work with: its name is grey on the switch
    /// and its tooltip says this (the same in every language, as its name is).
    pub fn stage(self) -> Option<&'static str> {
        match self {
            Product::Modeller => Some("(alpha, not usable)"),
            Product::Pointcloud | Product::Analysis | Product::Docs | Product::Gis | Product::Library => Some("(alpha)"),
            Product::Viewer | Product::Validator | Product::Ifcx => None,
        }
    }

    /// What it is for, as the switch's tooltip.
    pub fn purpose(self, l: Language) -> &'static str {
        match self {
            Product::Viewer => l.text("Modellen openen, bekijken, meten en doorsnijden", "Open, view, measure and cut models"),
            Product::Modeller => l.text("Een BIM-model maken uit wanden, vloeren, kolommen en balken", "Make a BIM model from walls, slabs, columns and beams"),
            Product::Validator => l.text("Modellen samen controleren en issues afspreken", "Check models together and agree on issues"),
            Product::Pointcloud => l.text("Puntenwolken openen en er meshes van maken", "Open point clouds and make meshes of them"),
            Product::Analysis => l.text("Rekenmodellen bekijken: belastingen en rekenresultaten", "Look at structural analysis models: loads and results"),
            Product::Docs => l.text("De documenten van het project: status, versies en koppelingen met het model", "The documents of the project: status, revisions and links to the model"),
            Product::Gis => l.text("GIS, infra en civiel: de kaart, de omgeving, het terrein en de ondergrond van het project", "GIS, infrastructure and civil: the map, the surroundings, the terrain and the ground of the project"),
            Product::Library => l.text("De bibliotheken: algemeen, per land, van het bedrijf en van het project", "The libraries: general, per country, of the company and of the project"),
            Product::Ifcx => l.text("Het project zoals het in IFCX staat: nodes, tabel, relaties en tekst", "The project as it stands in IFCX: nodes, table, relations and text"),
        }
    }

    /// Its command id, for the switch and a shortcut of one's own.
    pub fn command(self) -> &'static str {
        match self {
            Product::Viewer => "product.viewer",
            Product::Modeller => "product.modeller",
            Product::Validator => "product.validator",
            Product::Pointcloud => "product.pointcloud",
            Product::Analysis => "product.analysis",
            Product::Docs => "product.docs",
            Product::Gis => "product.gis",
            Product::Library => "product.library",
            Product::Ifcx => "product.ifcx",
        }
    }

    /// The tabs of its ribbon, the first shown when it opens.
    pub fn pages(self) -> &'static [RibbonPage] {
        match self {
            Product::Viewer => &[RibbonPage::Home],
            Product::Modeller => &[RibbonPage::Model, RibbonPage::Draw, RibbonPage::Project, RibbonPage::View],
            Product::Validator => &[RibbonPage::Collaborate, RibbonPage::View],
            Product::Pointcloud => &[RibbonPage::Cloud, RibbonPage::View],
            Product::Analysis => &[RibbonPage::Analysis, RibbonPage::View],
            Product::Docs => &[RibbonPage::Docs, RibbonPage::View],
            Product::Gis => &[RibbonPage::Gis, RibbonPage::Infra, RibbonPage::Civil, RibbonPage::Rail, RibbonPage::Geo, RibbonPage::View],
            // its page is no view of a model: it has its own tab only
            Product::Library => &[RibbonPage::Library],
            Product::Ifcx => &[RibbonPage::Ifcx],
        }
    }

    /// Its name on its own: the window title and About of a build of this product alone.
    pub fn name(self) -> &'static str {
        match self {
            Product::Viewer => "Open BIM Suite",
            Product::Modeller => "Open BIM Modeller",
            Product::Validator => "Open BIM Validator",
            Product::Pointcloud => "Open BIM Pointcloud",
            Product::Analysis => "Open BIM Structural Analysis",
            Product::Docs => "Open BIM Docs",
            Product::Gis => "Open BIM GIC",
            Product::Library => "Open BIM Library",
            Product::Ifcx => "Open BIM IFCX",
        }
    }

    /// Its id in the installer's catalogue, for the update notice of a build of it alone.
    pub fn app_id(self) -> &'static str {
        match self {
            Product::Viewer => "open-3d-viewer",
            Product::Modeller => "open-bim-modeller",
            Product::Validator => "open-bim-validator",
            Product::Pointcloud => "open-bim-pointcloud",
            Product::Analysis => "open-bim-analysis",
            Product::Docs => "open-bim-docs",
            Product::Gis => "open-bim-gis",
            Product::Library => "open-bim-library",
            Product::Ifcx => "open-bim-ifcx",
        }
    }

    /// Whether this build holds it: its Cargo feature.
    pub fn built(self) -> bool {
        match self {
            Product::Viewer => cfg!(feature = "viewer"),
            Product::Modeller => cfg!(feature = "modeller"),
            Product::Validator => cfg!(feature = "validator"),
            Product::Pointcloud => cfg!(feature = "pointcloud"),
            // part of the viewer: it only looks
            Product::Analysis => cfg!(feature = "viewer"),
            // in every desktop build: a project folder is a folder of the computer
            Product::Docs => cfg!(not(target_arch = "wasm32")),
            // part of the viewer: map layers and terrain are models like any other
            Product::Gis => cfg!(feature = "viewer"),
            // in every build: the libraries come with the suite itself
            Product::Library => true,
            // in every build: every project can be looked at as IFCX
            Product::Ifcx => true,
        }
    }

    /// Its place in [`Product::ALL`].
    pub fn index(self) -> usize {
        Product::ALL.iter().position(|&p| p == self).unwrap_or(0)
    }
}

#[cfg(not(any(feature = "viewer", feature = "modeller", feature = "validator", feature = "pointcloud")))]
compile_error!("a build needs at least one product: the feature viewer, modeller, validator or pointcloud");

/// What a build is: the Studio, with every product it holds and the switch between them, or one
/// product alone (`open-bim-modeller`, `open-bim-validator`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Edition {
    #[default]
    Studio,
    Only(Product),
}

impl Edition {
    /// The products it offers, in the order of the switch.
    pub fn products(self) -> Vec<Product> {
        match self {
            Edition::Studio => Product::ALL.into_iter().filter(|p| p.built()).collect(),
            Edition::Only(p) => vec![p],
        }
    }

    /// Its name: the window title and About.
    pub fn name(self) -> &'static str {
        match self {
            Edition::Studio => Product::Viewer.name(),
            Edition::Only(p) => p.name(),
        }
    }

    /// Its id in the installer's catalogue: the Studio keeps the viewer's, so installed copies keep
    /// updating.
    pub fn app_id(self) -> &'static str {
        match self {
            Edition::Studio => Product::Viewer.app_id(),
            Edition::Only(p) => p.app_id(),
        }
    }
}

/// A tab of a product's ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RibbonPage {
    /// The Viewer's one tab: file, display, camera, section, measure.
    Home,
    /// The Modeller's base elements, the openings in walls, joins and changes.
    Model,
    /// The Modeller's levels, views, sheets and library.
    Project,
    /// The Modeller's drawing in a floor plan, a section or on a sheet: selecting, the lines and
    /// shapes that are no model, dimensions and texts, and changing what is selected.
    Draw,
    /// Display, camera, section and measure, without the file group: the Viewer's tools in the
    /// Modeller and the Validator.
    View,
    /// The Validator's models, issues, checks and comments.
    Collaborate,
    /// The Modeller's IFCX worksheet: the whole project as its IFCX file has it, in place of the
    /// views.
    Ifcx,
    /// Pointcloud's clouds, the size of their points and the meshes made of them.
    Cloud,
    /// Structural Analysis: the internal forces, the deformed shape, reactions and loads.
    Analysis,
    /// Docs: the project folder, and opening and linking the picked document.
    Docs,
    /// GIS: the map, the surroundings and the terrain of the place.
    Gis,
    /// Geotechnics: the ground under the project as it was investigated.
    Geo,
    /// Infrastructure: roads, pipes and cables. Its tools are still to come.
    Infra,
    /// Civil engineering: bridges, viaducts, tunnels and other structures. Its tools are still to come.
    Civil,
    /// Rail: tracks, turnouts and clearance. Its tools are still to come.
    Rail,
    /// Library: the source whose library shows.
    Library,
}

impl RibbonPage {
    pub fn label(self, l: Language) -> &'static str {
        match self {
            RibbonPage::Home => l.text("Start", "Home"),
            RibbonPage::Model => l.text("Modelleren", "Model"),
            RibbonPage::Project => l.text("Project", "Project"),
            RibbonPage::Draw => l.text("Tekenen", "Draw"),
            RibbonPage::View => l.text("Beeld", "View"),
            RibbonPage::Collaborate => l.text("Samenwerken", "Collaborate"),
            RibbonPage::Ifcx => "IFCX",
            RibbonPage::Cloud => l.text("Puntenwolk", "Point cloud"),
            RibbonPage::Analysis => l.text("Rekenmodel", "Analysis"),
            RibbonPage::Docs => "Docs",
            RibbonPage::Gis => "GIS",
            RibbonPage::Geo => l.text("Geotechniek", "Geotechnics"),
            RibbonPage::Infra => "Infra",
            RibbonPage::Civil => l.text("Civiel", "Civil"),
            RibbonPage::Rail => "Rail",
            RibbonPage::Library => l.text("Bibliotheek", "Library"),
        }
    }
}

/// A command of the Validator's ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Planned {
    pub id: &'static str,
    pub icon: &'static str,
}

/// The Validator's commands, by ribbon group: issues (BCF), checks (IDS and the built-in
/// ones), clashes and the rule set that holds them.
pub const PLANNED: [Planned; 9] = [
    Planned { id: "collaborate.issue", icon: "git.issue" },
    Planned { id: "collaborate.bcf-import", icon: "bcf.import" },
    Planned { id: "collaborate.bcf-export", icon: "bcf.export" },
    Planned { id: "collaborate.check", icon: "validation.workspace" },
    Planned { id: "collaborate.ids", icon: "validation.select-specification" },
    Planned { id: "collaborate.report", icon: "export.ifc-report" },
    Planned { id: "collaborate.clash", icon: "ifc.compare-files" },
    Planned { id: "collaborate.rules-open", icon: "validation.select-specification" },
    Planned { id: "collaborate.rules-save", icon: "file.save-as" },
];

impl Planned {
    pub fn label(self, l: Language) -> &'static str {
        match self.id {
            "collaborate.issue" => l.text("Nieuw issue", "New issue"),
            "collaborate.bcf-import" => l.text("BCF openen", "Open BCF"),
            "collaborate.bcf-export" => l.text("BCF opslaan", "Save BCF"),
            "collaborate.ids" => l.text("IDS laden", "Load IDS"),
            "collaborate.check" => l.text("Model controleren", "Check model"),
            "collaborate.report" => l.text("Rapport opslaan", "Save report"),
            "collaborate.clash" => l.text("Clashes zoeken", "Find clashes"),
            "collaborate.rules-open" => l.text("Regelset openen", "Open rule set"),
            _ => l.text("Regelset opslaan", "Save rule set"),
        }
    }

    /// What it does, as its tooltip.
    pub fn detail(self, l: Language) -> &'static str {
        match self.id {
            "collaborate.issue" => l.text("Een issue van de weergave zoals die nu is: camera, selectie, wat verborgen is en een afbeelding", "An issue of the view as it is: camera, selection, what is hidden and a picture"),
            "collaborate.bcf-import" => l.text("Issues openen uit een BCF-bestand (2.0, 2.1 of 3.0)", "Open issues from a BCF file (2.0, 2.1 or 3.0)"),
            "collaborate.bcf-export" => l.text("De issues opslaan als BCF 2.1", "Save the issues as BCF 2.1"),
            "collaborate.ids" => l.text("Een IDS toevoegen aan de regelset", "Add an IDS to the rule set"),
            "collaborate.check" => l.text("De IFC-modellen controleren op de regelset", "Check the IFC models on the rule set"),
            "collaborate.report" => l.text("Het resultaat van de controle opslaan als HTML-rapport", "Save what the check found as an HTML report"),
            "collaborate.clash" => l.text("De clashtests van de regelset uitvoeren op de geopende modellen", "Run the clash tests of the rule set on the open models"),
            "collaborate.rules-open" => l.text("Een regelset openen: controles en clashtests in één bestand", "Open a rule set: checks and clash tests in one file"),
            _ => l.text("De regelset opslaan om te delen met het project", "Save the rule set to share with the project"),
        }
    }

    /// What it sets off in the project.
    #[cfg(all(feature = "validator", not(target_arch = "wasm32")))]
    pub fn event(self) -> Option<crate::app::collab::Event> {
        use crate::app::collab::{Event, File, Report};
        Some(match self.id {
            "collaborate.issue" => Event::NewIssue,
            "collaborate.bcf-import" => Event::Open(File::Bcf),
            "collaborate.bcf-export" => Event::SaveIssues,
            "collaborate.ids" => Event::Open(File::Ids),
            "collaborate.check" => Event::Check,
            "collaborate.report" => Event::Export(Report::Html),
            "collaborate.clash" => Event::Clash,
            "collaborate.rules-open" => Event::Open(File::Rules),
            "collaborate.rules-save" => Event::SaveRules,
            _ => return None,
        })
    }
}

/// The Validator's left panel: the project's models, its issues and its checks. Issues and
/// checks are still to come; until then the panel says what they will hold.
pub fn collaboration_panel<'a, M: 'a>(look: Look, l: Language, models: Vec<&'a str>) -> Element<'a, M> {
    use iced::widget::{column, row, scrollable, Space};
    use iced::{Alignment, Length};
    use openaec_ui::design::widgets as w;
    let heading = |title: &'a str, count: Option<usize>| -> Element<'a, M> {
        let mut line = row![w::heading(look, title, 13.0).width(Length::Fill)].align_y(Alignment::Center);
        if let Some(n) = count {
            line = line.push(w::secondary(look, n.to_string()));
        }
        line.into()
    };
    let mut open = column![].spacing(2);
    if models.is_empty() {
        open = open.push(w::secondary(look, l.text("Nog geen modellen geopend.", "No models open yet.")));
    }
    for name in &models {
        open = open.push(w::body(look, *name));
    }
    let body = column![
        heading(l.text("Modellen", "Models"), Some(models.len())),
        open,
        Space::new().height(10),
        heading(l.text("Issues", "Issues"), None),
        w::secondary(
            look,
            l.text(
                "Nog geen issues. Hier komen de BCF-issues van het project: een standpunt in het model, opmerkingen, een status en wie het oppakt.",
                "No issues yet. The project's BCF issues will be here: a viewpoint in the model, comments, a status and who takes it on.",
            )
        ),
        Space::new().height(10),
        heading(l.text("Controles", "Checks"), None),
        w::secondary(
            look,
            l.text(
                "Nog geen controles. Hier komt het controleren van de modellen tegen een IDS: wat voldoet, wat niet, en wat niet te controleren is.",
                "No checks yet. Checking the models against an IDS will be here: what passes, what fails and what cannot be checked.",
            )
        ),
    ]
    .spacing(6);
    scrollable(body).height(Length::Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_product_has_a_key_that_reads_back_and_a_ribbon() {
        for p in Product::ALL {
            assert_eq!(Product::parse(p.key()), Some(p));
            assert_eq!(Product::parse(&p.key().to_uppercase()), Some(p));
            assert!(!p.pages().is_empty(), "{p:?}");
            assert_eq!(Product::ALL[p.index()], p);
        }
        assert_eq!(Product::parse("modeler"), Some(Product::Modeller));
        assert_eq!(Product::parse("editor"), None);
        // the switch: short names, and the four in alpha are the grey ones
        assert_eq!(Product::ALL.map(Product::label), ["View", "IFCX", "Validator", "PointCloud", "Struct", "Modeller", "Docs", "GIC", "Library"]);
        assert_eq!(Product::ALL.map(|p| p.stage().is_some()), [false, false, false, true, true, true, true, true, true]);
    }

    #[test]
    fn every_product_alone_has_its_own_name_and_installer_id() {
        let mut names: Vec<&str> = Product::ALL.iter().map(|p| p.name()).collect();
        let mut ids: Vec<&str> = Product::ALL.iter().map(|p| p.app_id()).collect();
        names.sort();
        names.dedup();
        ids.sort();
        ids.dedup();
        assert_eq!((names.len(), ids.len()), (9, 9));
        // the Studio is what was released as the viewer: same name, same installer id
        assert_eq!((Edition::Studio.name(), Edition::Studio.app_id()), ("Open BIM Suite", "open-3d-viewer"));
        assert_eq!(Edition::Only(Product::Modeller).products(), vec![Product::Modeller]);
    }

    #[test]
    fn the_studio_offers_the_products_of_the_build() {
        let products = Edition::Studio.products();
        assert!(!products.is_empty());
        for p in Product::ALL {
            assert_eq!(products.contains(&p), p.built(), "{p:?}");
        }
    }

    #[test]
    fn the_viewer_has_no_modelling_and_the_modeller_starts_with_it() {
        assert!(!Product::Viewer.pages().contains(&RibbonPage::Model));
        assert_eq!(Product::Modeller.pages()[0], RibbonPage::Model);
        assert_eq!(Product::Validator.pages()[0], RibbonPage::Collaborate);
        assert_eq!(Product::Pointcloud.pages()[0], RibbonPage::Cloud);
        assert_eq!(Product::Docs.pages(), [RibbonPage::Docs, RibbonPage::View]);
    }

    #[test]
    fn a_product_reads_from_the_preferences_by_its_key() {
        for p in Product::ALL {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{}\"", p.key()));
            assert_eq!(serde_json::from_str::<Product>(&json).unwrap(), p);
        }
        // the Validator under its earlier name, in preferences and on the command line
        assert_eq!(serde_json::from_str::<Product>("\"collaborator\"").unwrap(), Product::Validator);
        assert_eq!(Product::parse("collaborator"), Some(Product::Validator));
    }

    #[test]
    fn planned_commands_have_their_own_names() {
        let l = Language::English;
        let mut names: Vec<&str> = PLANNED.iter().map(|p| p.label(l)).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), PLANNED.len());
    }
}
