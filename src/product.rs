//! The three products built from this code base: the Viewer, the Modeller and the BIM
//! Collaborator (see `docs/architecture/products.md`).
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
    /// Check models together and agree on issues.
    Collaborator,
}

impl Product {
    pub const ALL: [Product; 3] = [Product::Viewer, Product::Modeller, Product::Collaborator];

    /// Its key on the command line, in the preferences and in a web address (`?product=`).
    pub fn key(self) -> &'static str {
        match self {
            Product::Viewer => "viewer",
            Product::Modeller => "modeller",
            Product::Collaborator => "collaborator",
        }
    }

    /// The product of a key, in any case; `modeler` too.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Some(Product::Viewer),
            "modeller" | "modeler" => Some(Product::Modeller),
            "collaborator" | "bim-collaborator" => Some(Product::Collaborator),
            _ => None,
        }
    }

    /// Its name on the switch: a product name, the same in every language.
    pub fn label(self) -> &'static str {
        match self {
            Product::Viewer => "Viewer",
            Product::Modeller => "Modeller",
            Product::Collaborator => "BIM Collaborator",
        }
    }

    /// How far along it is, said beside its name on the switch where it is not ready to work
    /// with (the same in every language, as its name is).
    pub fn stage(self) -> Option<&'static str> {
        match self {
            Product::Modeller => Some("(alpha, not usable)"),
            Product::Viewer | Product::Collaborator => None,
        }
    }

    /// What it is for, as the switch's tooltip.
    pub fn purpose(self, l: Language) -> &'static str {
        match self {
            Product::Viewer => l.text("Modellen openen, bekijken, meten en doorsnijden", "Open, view, measure and cut models"),
            Product::Modeller => l.text("Een BIM-model maken uit wanden, vloeren, kolommen en balken", "Make a BIM model from walls, slabs, columns and beams"),
            Product::Collaborator => l.text("Modellen samen controleren en issues afspreken", "Check models together and agree on issues"),
        }
    }

    /// Its command id, for the switch and a shortcut of one's own.
    pub fn command(self) -> &'static str {
        match self {
            Product::Viewer => "product.viewer",
            Product::Modeller => "product.modeller",
            Product::Collaborator => "product.collaborator",
        }
    }

    /// The tabs of its ribbon, the first shown when it opens.
    pub fn pages(self) -> &'static [RibbonPage] {
        match self {
            Product::Viewer => &[RibbonPage::Home],
            Product::Modeller => &[RibbonPage::Model, RibbonPage::Project, RibbonPage::Annotate, RibbonPage::View],
            Product::Collaborator => &[RibbonPage::Collaborate, RibbonPage::View],
        }
    }

    /// Its name on its own: the window title and About of a build of this product alone.
    pub fn name(self) -> &'static str {
        match self {
            Product::Viewer => "Open BIM Suite",
            Product::Modeller => "Open BIM Modeller",
            Product::Collaborator => "Open BIM Collaborator",
        }
    }

    /// Its id in the installer's catalogue, for the update notice of a build of it alone.
    pub fn app_id(self) -> &'static str {
        match self {
            Product::Viewer => "open-3d-viewer",
            Product::Modeller => "open-bim-modeller",
            Product::Collaborator => "open-bim-collaborator",
        }
    }

    /// Whether this build holds it: its Cargo feature.
    pub fn built(self) -> bool {
        match self {
            Product::Viewer => cfg!(feature = "viewer"),
            Product::Modeller => cfg!(feature = "modeller"),
            Product::Collaborator => cfg!(feature = "collaborator"),
        }
    }

    /// Its place in [`Product::ALL`].
    pub fn index(self) -> usize {
        Product::ALL.iter().position(|&p| p == self).unwrap_or(0)
    }
}

#[cfg(not(any(feature = "viewer", feature = "modeller", feature = "collaborator")))]
compile_error!("a build needs at least one product: the feature viewer, modeller or collaborator");

/// What a build is: the Studio, with every product it holds and the switch between them, or one
/// product alone (`open-bim-modeller`, `open-bim-collaborator`).
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
    /// The Modeller's dimensions, texts and sections, in a floor plan or on a sheet.
    Annotate,
    /// Display, camera, section and measure, without the file group: the Viewer's tools in the
    /// Modeller and the Collaborator.
    View,
    /// The Collaborator's models, issues, checks and comments.
    Collaborate,
}

impl RibbonPage {
    pub fn label(self, l: Language) -> &'static str {
        match self {
            RibbonPage::Home => l.text("Start", "Home"),
            RibbonPage::Model => l.text("Modelleren", "Model"),
            RibbonPage::Project => l.text("Project", "Project"),
            RibbonPage::Annotate => l.text("Annoteren", "Annotate"),
            RibbonPage::View => l.text("Beeld", "View"),
            RibbonPage::Collaborate => l.text("Samenwerken", "Collaborate"),
        }
    }
}

/// A command of the Collaborator's ribbon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Planned {
    pub id: &'static str,
    pub icon: &'static str,
}

/// The Collaborator's commands, by ribbon group: issues (BCF), checks (IDS and the built-in
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
    #[cfg(all(feature = "collaborator", not(target_arch = "wasm32")))]
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

/// The Collaborator's left panel: the project's models, its issues and its checks. Issues and
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
    }

    #[test]
    fn every_product_alone_has_its_own_name_and_installer_id() {
        let mut names: Vec<&str> = Product::ALL.iter().map(|p| p.name()).collect();
        let mut ids: Vec<&str> = Product::ALL.iter().map(|p| p.app_id()).collect();
        names.sort();
        names.dedup();
        ids.sort();
        ids.dedup();
        assert_eq!((names.len(), ids.len()), (3, 3));
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
        assert_eq!(Product::Collaborator.pages()[0], RibbonPage::Collaborate);
    }

    #[test]
    fn a_product_reads_from_the_preferences_by_its_key() {
        for p in Product::ALL {
            let json = serde_json::to_string(&p).unwrap();
            assert_eq!(json, format!("\"{}\"", p.key()));
            assert_eq!(serde_json::from_str::<Product>(&json).unwrap(), p);
        }
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
