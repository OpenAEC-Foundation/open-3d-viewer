//! The material editor of the Library product: the materials of the project's library.
//!
//! The Project page shows the materials as chips, each with its hatch and its colour; a click
//! picks one, and the right pane is its editor: its name in the library's four languages, its
//! colour in the 3D model (typed as six hexadecimal digits, or set a channel at a time) and the
//! hatch a cut through it is drawn with, picked from the hatches drawn. Its key stays what it is:
//! layers, types and files refer to it. A material is `IfcMaterial` in a file.
//!
//! The editor also chooses the hatch standard the project hatches by, adds one as a copy of the
//! chosen one, and sets the material's hatch under it: of a cut, of a face seen, the scale it is
//! drawn from and what a smaller drawing shows (`bim_library::standard`).
//!
//! A material that a build-up, a linear type or a system type is made of is not removed. A
//! material of another library is edited as a copy in the project, under the key it had.
//!
//! The model takes a material's colour as it is. A material hatched as air is a cavity in the
//! walls and slabs made of it: they follow when its hatch changes (see `type_follow`).

use super::libraries::{Category, Editor, Shelf};
use super::{App, Message, FONT_BOLD};
use crate::theme::Palette;
use bim_library::{Below, DrawingStandard, HatchKind, HatchStandard, Lang, Library, LibraryError, Material, Names};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, pick_list, row, slider, text, text_input, Column, Space};
use iced::{Center, Color, Element, Fill, Length, Task};
use openaec_ui::design::a11y::{self, Role as Aria};
use openaec_ui::Language;

#[derive(Debug, Clone)]
pub enum MaterialMessage {
    /// Edit this material of the project.
    Pick(String),
    /// A material more, to be named.
    Add,
    Duplicate,
    Remove,
    /// Typed as its name in a language.
    Named(Lang, String),
    /// Typed as its colour: "#b9b9b9".
    Hex(String),
    /// Its red, green or blue (0, 1, 2) set.
    Channel(usize, u8),
    /// A slider is let go: the model takes the colour.
    Shown,
    Hatch(HatchKind),
    /// Typed as its grade or strength class ("C24"); empty for none.
    Graded(String),
    /// Its own hatch of the hatch catalogue, by its key; none: drawn by its hatch kind.
    Sign(Option<String>),
    /// Edit a material of another library: a copy of it in the project, under its key.
    Copy(Shelf, String),
    /// The hatch standard the project hatches by, by its key.
    Standard(String),
    /// A standard more: a copy of the chosen one, to be named, and chosen.
    AddStandard,
    /// Typed as the name of the chosen standard.
    StandardNamed(String),
    /// Under the chosen standard: the hatch of a cut through the material, by its key; none:
    /// by its hatch kind.
    Cut(Option<String>),
    /// Under the chosen standard: the hatch of a face of it that is seen; none: no hatch.
    Face(Option<String>),
    /// Typed as the scale from which its hatch is drawn ("100" for 1 : 100); empty: always.
    From(String),
    /// What a smaller drawing shows.
    Below(Below),
    /// Under the chosen standard it names no hatch of its own any more: its sign gives it.
    Usual,
}

/// Which material is edited, what is being typed, and how many types held a removal back.
#[derive(Debug, Clone, Default)]
pub struct MaterialView {
    pub picked: Option<String>,
    /// A name as it is being typed, in its language: a name cannot be empty, its field can.
    pub name: Option<(Lang, String)>,
    /// A colour as it is being typed.
    pub hex: Option<String>,
    /// How many types are made of the material a removal was asked of.
    pub in_use: Option<usize>,
    /// The scale its hatch is drawn from, as it is being typed.
    pub from: Option<String>,
    /// The name of the chosen hatch standard, as it is being typed.
    pub standard: Option<String>,
}

/// A colour as it is typed: "#b9b9b9".
pub fn hex(colour: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", colour[0], colour[1], colour[2])
}

/// The colour typed as six hexadecimal digits, with or without "#".
pub fn colour_of(typed: &str) -> Option<[u8; 3]> {
    let digits = typed.trim().trim_start_matches('#');
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |k: usize| u8::from_str_radix(&digits[2 * k..2 * k + 2], 16).ok();
    Some([channel(0)?, channel(1)?, channel(2)?])
}

/// A language of the library's names, as it names itself.
pub(super) fn lang_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Nl => "Nederlands",
        Lang::En => "English",
        Lang::De => "Deutsch",
        Lang::Fr => "Français",
    }
}

fn name_mut(names: &mut Names, lang: Lang) -> &mut String {
    match lang {
        Lang::Nl => &mut names.nl,
        Lang::En => &mut names.en,
        Lang::De => &mut names.de,
        Lang::Fr => &mut names.fr,
    }
}

/// The material of the general library a hatch kind is named after: brickwork, concrete, ...
pub(super) fn hatch_material(kind: HatchKind) -> &'static str {
    match kind {
        HatchKind::Brick => "brick",
        HatchKind::SandLime => "sand-lime",
        HatchKind::Concrete => "concrete",
        HatchKind::Timber => "timber",
        HatchKind::Steel => "steel",
        HatchKind::Insulation => "insulation",
        HatchKind::Gypsum => "gypsum-board",
        HatchKind::Air => "air",
        HatchKind::Generic => "generic",
    }
}

/// Changes material `key` of `library`; whether it is there.
pub fn change(library: &mut Library, key: &str, change: impl FnOnce(&mut Material)) -> bool {
    library.materials.iter_mut().find(|m| m.key == key).map(change).is_some()
}

/// A key no material of the library has: "own-material-1".
fn fresh_key(library: &Library) -> String {
    (1..).map(|n| format!("own-material-{n}")).find(|key| library.material(key).is_none()).unwrap_or_default()
}

/// A material more, grey and with the general hatch, to be named; its key.
pub fn add(library: &mut Library) -> Result<String, LibraryError> {
    let key = fresh_key(library);
    library.add_material(Material::new(&key, Names::new("nieuw materiaal", "new material", "neues Material", "nouveau matériau"), HatchKind::Generic, [175, 182, 195]))?;
    Ok(key)
}

/// A copy of material `key`, right after it, its names told apart by a 2; the copy's key.
pub fn duplicate(library: &mut Library, key: &str) -> Result<String, LibraryError> {
    let at = library.materials.iter().position(|m| m.key == key).ok_or(LibraryError::NoKey)?;
    let mut copy = library.materials[at].clone();
    copy.key = fresh_key(library);
    for lang in Lang::ALL {
        name_mut(&mut copy.names, lang).push_str(" 2");
    }
    let new = copy.key.clone();
    library.materials.insert(at + 1, copy);
    Ok(new)
}

/// Material `key` of `from` in `library`, under the same key: the one it has, or a copy.
pub fn copy(library: &mut Library, from: &Library, key: &str) -> Result<String, LibraryError> {
    if library.material(key).is_none() {
        library.add_material(from.material(key).ok_or(LibraryError::NoKey)?.clone())?;
    }
    Ok(key.to_string())
}

/// How many build-ups, linear types and system types of the library are made of material `key`.
pub fn users(library: &Library, key: &str) -> [usize; 3] {
    let rest: Vec<Material> = library.materials.iter().filter(|m| m.key != key).cloned().collect();
    [
        library.buildups.iter().filter(|b| b.layers.iter().any(|layer| layer.material == key)).count(),
        library.linear_types.iter().filter(|t| t.material == key).count(),
        // (a system names its materials in several places: it uses one it does not check without)
        library.system_types.iter().filter(|t| t.check(&library.materials).is_ok() && t.check(&rest).is_err()).count(),
    ]
}

/// Takes material `key` out of the library; refused, with how many types are made of it, while
/// one is.
pub fn remove(library: &mut Library, key: &str) -> Result<(), usize> {
    let used: usize = users(library, key).iter().sum();
    if used > 0 {
        return Err(used);
    }
    library.materials.retain(|m| m.key != key);
    Ok(())
}

/// The names a standard more starts with.
fn own_standard() -> Names {
    Names::new("Eigen standaard", "Own standard", "Eigener Standard", "Norme propre")
}

/// Whether a standard is one a library starts with: those keep their names.
fn given(standard: &HatchStandard) -> bool {
    bim_library::standard::starter().iter().any(|s| s.key == standard.key)
}

/// Changes the hatch of material `key` under the chosen standard of `library`: what it has
/// there now (its own, else what its sign gives) as `change` leaves it. False when the library
/// lacks the material or a standard, or the result does not hold together.
fn rehatch(library: &mut Library, key: &str, change: impl FnOnce(&mut bim_library::MaterialHatch)) -> bool {
    let Some((standard, mut own)) = library.hatch_standard().and_then(|s| Some((s.key.clone(), s.of(library.material(key)?)))) else { return false };
    change(&mut own);
    library.set_material_hatch(key, &standard, Some(own)).is_ok()
}

/// A scale as it is typed: "100", "1:100" or "1 : 100"; empty: 0, at every scale.
pub fn scale_of(typed: &str) -> Option<f64> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Some(0.0);
    }
    let number = typed.strip_prefix('1').map(str::trim_start).and_then(|rest| rest.strip_prefix(':')).unwrap_or(typed).trim().replace(',', ".");
    number.parse::<f64>().ok().filter(|scale| scale.is_finite() && *scale >= 0.0)
}

/// A field of the right pane: its label, and beside it what sets it.
fn labelled<'a>(p: Palette, label: &'static str, control: Element<'a, MaterialMessage>) -> Element<'a, MaterialMessage> {
    row![text(label).size(12).color(p.text_secondary).wrapping(Wrapping::WordOrGlyph).width(Length::FillPortion(2)), container(control).width(Length::FillPortion(3))].spacing(8).align_y(Center).into()
}

/// A hatch of the catalogue to pick for a material, or none.
#[derive(Debug, Clone, PartialEq)]
struct SignChoice {
    key: Option<String>,
    label: String,
}

/// A hatch standard to choose.
#[derive(Debug, Clone, PartialEq)]
struct StandardChoice {
    key: String,
    label: String,
}

impl std::fmt::Display for StandardChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl std::fmt::Display for SignChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

impl App {
    pub(super) fn material_update(&mut self, message: MaterialMessage) -> Task<Message> {
        self.follow_refused.clear();
        let picked = self.material_view.picked.clone();
        // whether the library changed, and whether the model is to show it now
        let (mut changed, mut show) = (false, true);
        match message {
            MaterialMessage::Pick(key) => self.material_show(key),
            MaterialMessage::Add => {
                if let Ok(key) = add(&mut self.library) {
                    self.material_view = MaterialView { picked: Some(key), ..MaterialView::default() };
                    changed = true;
                }
            }
            MaterialMessage::Duplicate => {
                if let Some(Ok(new)) = picked.as_deref().map(|key| duplicate(&mut self.library, key)) {
                    self.material_view = MaterialView { picked: Some(new), ..MaterialView::default() };
                    changed = true;
                }
            }
            MaterialMessage::Remove => {
                if let Some(key) = &picked {
                    match remove(&mut self.library, key) {
                        Ok(()) => {
                            self.material_view = MaterialView::default();
                            changed = true;
                        }
                        Err(used) => self.material_view.in_use = Some(used),
                    }
                }
            }
            MaterialMessage::Named(lang, typed) => {
                let name = typed.trim().to_string();
                if let (Some(key), false) = (&picked, name.is_empty()) {
                    changed = change(&mut self.library, key, |m| *name_mut(&mut m.names, lang) = name);
                }
                // (what was typed stays in its field: a space at its end, or nothing at all)
                self.material_view.name = Some((lang, typed));
            }
            MaterialMessage::Hex(typed) => {
                if let (Some(key), Some(colour)) = (&picked, colour_of(&typed)) {
                    changed = change(&mut self.library, key, |m| m.colour = colour);
                }
                self.material_view.hex = Some(typed);
            }
            MaterialMessage::Channel(channel, value) => {
                if let Some(key) = &picked {
                    changed = change(&mut self.library, key, |m| {
                        if let Some(of) = m.colour.get_mut(channel) {
                            *of = value;
                        }
                    });
                }
                self.material_view.hex = None;
                // a slider that is dragged sets many values: the model follows when it is let go
                show = false;
            }
            MaterialMessage::Shown => changed = picked.is_some(),
            MaterialMessage::Graded(grade) => {
                if let Some(key) = &picked {
                    let grade = Some(grade.trim().to_string()).filter(|grade| !grade.is_empty());
                    changed = change(&mut self.library, key, |m| m.grade = grade);
                    // (a grade changes no drawing: the model is not made again)
                    show = false;
                }
            }
            MaterialMessage::Sign(sign) => {
                if let Some(key) = &picked {
                    changed = change(&mut self.library, key, |m| m.sign = sign.filter(|key| bim_library::hatch::hatch(key).is_some()));
                }
            }
            MaterialMessage::Hatch(hatch) => {
                if let Some(key) = &picked {
                    changed = change(&mut self.library, key, |m| m.hatch = hatch);
                    // air is a cavity in a wall: the walls and slabs made of it follow
                    self.follow_material(key);
                }
            }
            MaterialMessage::Standard(key) => {
                changed = self.library.hatch_standard().is_some_and(|s| s.key != key) && self.library.choose_hatch_standard(&key);
                (self.material_view.from, self.material_view.standard) = (None, None);
            }
            MaterialMessage::AddStandard => {
                if self.library.hatch_standards.is_empty() {
                    self.library.hatch_standards = bim_library::standard::starter();
                }
                let from = self.library.hatch_standard().map(|s| s.key.clone()).unwrap_or_default();
                if let Ok(key) = self.library.copy_hatch_standard(&from, own_standard()) {
                    changed = self.library.choose_hatch_standard(&key);
                    (self.material_view.from, self.material_view.standard) = (None, None);
                }
            }
            MaterialMessage::StandardNamed(typed) => {
                let name = typed.trim().to_string();
                if let Some(s) = self.library.hatch_standards.iter_mut().find(|s| s.chosen).filter(|s| !given(s) && !name.is_empty()) {
                    s.names = Names::same(&name);
                    changed = true;
                    // (a name changes no drawing)
                    show = false;
                }
                self.material_view.standard = Some(typed);
            }
            MaterialMessage::Cut(cut) => {
                if let Some(key) = &picked {
                    changed = rehatch(&mut self.library, key, |own| own.cut = cut);
                }
            }
            MaterialMessage::Face(face) => {
                if let Some(key) = &picked {
                    changed = rehatch(&mut self.library, key, |own| own.face = face);
                    // (a face seen is not hatched on a drawing yet)
                    show = false;
                }
            }
            MaterialMessage::From(typed) => {
                if let (Some(key), Some(scale)) = (&picked, scale_of(&typed)) {
                    changed = rehatch(&mut self.library, key, |own| own.from = scale);
                }
                self.material_view.from = Some(typed);
            }
            MaterialMessage::Below(below) => {
                if let Some(key) = &picked {
                    changed = rehatch(&mut self.library, key, |own| own.below = below);
                }
            }
            MaterialMessage::Usual => {
                if let (Some(key), Some(standard)) = (&picked, self.library.hatch_standard().map(|s| s.key.clone())) {
                    changed = self.library.material(key).is_some_and(|m| m.hatches.contains_key(&standard)) && self.library.set_material_hatch(key, &standard, None).is_ok();
                    self.material_view.from = None;
                }
            }
            MaterialMessage::Copy(shelf, key) => {
                let from = self.shelf(shelf).clone();
                if let Ok(key) = copy(&mut self.library, &from, &key) {
                    self.material_show(key);
                    changed = true;
                }
            }
        }
        if changed {
            self.material_view.in_use = None;
            // what the model shows of its materials (their colours, the names of its types) is
            // made again
            if show && self.modeller.shown.is_some() && self.modeller.model.iter().next().is_some() {
                self.show_modelled();
            }
        }
        if self.material_view.picked.is_some() {
            self.edit_only(Editor::Material);
        }
        Task::none()
    }

    /// A material of the project in its editor, on the project's page (from the page or a search).
    fn material_show(&mut self, key: String) {
        self.material_view = MaterialView { picked: Some(key), ..MaterialView::default() };
        self.show_in_project(Category::Materials);
    }

    /// The right pane of the Project page while a material is picked: its editor.
    pub fn material_pane(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let lang = super::library::lang(l);
        let view = &self.material_view;
        let library = &self.library;
        let Some(m) = view.picked.as_deref().and_then(|key| library.material(key)) else {
            let hint = l.text("Kies een materiaal op de pagina om het te bewerken.", "Pick a material on the page to edit it.");
            return container(text(hint).size(12).color(p.text_muted).wrapping(Wrapping::Word)).padding(8).into();
        };
        let caption = |caption: &'static str| text(caption).size(11).color(p.text_muted);
        // how it looks: the hatch of a cut through it, and its colour in the 3D model
        let [r, g, b] = m.colour;
        let solid = container(Space::new().width(Fill).height(60)).style(move |_| Palette::swatch(Color::from_rgb8(r, g, b)));
        let looks = row![
            column![caption(l.text("Doorsnede", "Section")), super::library::material_hatch(p, m)].spacing(2).width(Length::FillPortion(3)),
            column![caption(l.text("3D-model", "3D model")), solid].spacing(2).width(Length::FillPortion(2)),
        ]
        .spacing(8);
        let mut fields = Column::new().spacing(6);
        // its name in each language of the library
        for of in Lang::ALL {
            let now = match &view.name {
                Some((typed_in, typed)) if *typed_in == of => typed.as_str(),
                _ => m.name(of),
            };
            let input = text_input("", now).on_input(move |typed| MaterialMessage::Named(of, typed)).size(12).padding([2, 6]);
            fields = fields.push(labelled(p, lang_name(of), a11y::named(input, Aria::TextField, lang_name(of))));
        }
        // its colour: typed, or a channel at a time
        let colour_name = l.text("Kleur", "Colour");
        let typed = view.hex.clone().unwrap_or_else(|| hex(m.colour));
        fields = fields.push(labelled(p, colour_name, a11y::named(text_input("#b9b9b9", &typed).on_input(MaterialMessage::Hex).size(12).padding([2, 6]), Aria::TextField, colour_name)));
        for (channel, name) in ["R", "G", "B"].into_iter().enumerate() {
            let value = m.colour[channel];
            let set = a11y::named(slider(0..=255, value, move |value| MaterialMessage::Channel(channel, value)).on_release(MaterialMessage::Shown).width(Fill), Aria::Slider, name);
            fields = fields.push(row![text(name).size(11).color(p.text_muted).width(12), set, text(value.to_string()).size(11).color(p.text_secondary).width(26)].spacing(6).align_y(Center));
        }
        // its hatch: one of the kinds, each drawn as a section draws it and named after the
        // general library's material of that kind
        let general = self.shelf(Shelf::General);
        let hatches = row(HatchKind::ALL.map(|kind| {
            let on = m.hatch == kind;
            let name = general.material(hatch_material(kind)).map_or("", |named| named.name(lang));
            // (three to a row of the pane; a long name stays inside its tile)
            let tile = column![super::libraries::hatch_tile(kind.pattern(DrawingStandard::Nen47), p), text(name).size(10).color(if on { p.text } else { p.text_secondary }).wrapping(Wrapping::None)].spacing(2).align_x(Center).width(70);
            button(container(tile).clip(true)).padding(4).style(move |_, s| if on { p.ribbon_button(true, s) } else { p.row_button(s) }).on_press(MaterialMessage::Hatch(kind)).into()
        }))
        .spacing(2)
        .wrap();
        fields = fields.push(caption(l.text("Arcering", "Hatch"))).push(hatches);
        // its grade, and what a timber class says of it
        let grade_name = l.text("Kwaliteit", "Grade");
        let grade = text_input("C24", m.grade.as_deref().unwrap_or_default()).on_input(MaterialMessage::Graded).size(12).padding([2, 6]);
        fields = fields.push(labelled(p, grade_name, a11y::named(grade, Aria::TextField, grade_name)));
        if let Some(class) = m.timber_grade() {
            let strength = format!("fm,k {} · ft,0,k {} · fc,0,k {} · fv,k {} N/mm²", class.fm_k, class.ft_0_k, class.fc_0_k, class.fv_k);
            let stiffness = format!("E0,mean {} · Gmean {} N/mm² · ρk {} kg/m³", class.e_0_mean, class.g_mean, class.density());
            fields = fields.push(text(strength).size(11).color(p.text_secondary).wrapping(Wrapping::Word)).push(text(stiffness).size(11).color(p.text_secondary).wrapping(Wrapping::Word));
        }
        // its own hatch of the catalogue: the sign of its kind of material, in its colour
        let own_hatch = l.text("Eigen arcering", "Hatch of its own");
        let by_kind = SignChoice { key: None, label: l.text("volgens soort", "by its kind").to_string() };
        let signs: Vec<SignChoice> = std::iter::once(by_kind)
            .chain(bim_library::hatch::of(bim_library::HatchSet::OpenAec).map(|h| SignChoice { key: Some(h.key.clone()), label: format!("{} · {}", h.sign, h.name(lang)) }))
            .collect();
        let now = signs.iter().find(|c| c.key == m.sign).cloned();
        let pick = pick_list(signs, now, |c: SignChoice| MaterialMessage::Sign(c.key)).text_size(12).padding([2, 6]).width(Fill);
        fields = fields.push(labelled(p, own_hatch, a11y::named(pick, Aria::ComboBox, own_hatch)));
        if let Some(hatch) = m.catalogue_hatch() {
            fields = fields.push(super::libraries::tile_hatch_look(hatch, p, 56.0, super::libraries::HATCH_LOOK_SCALE));
        }
        // the hatch standard the project hatches by, and the material's hatch under it
        let standards: Vec<StandardChoice> = library.hatch_standards.iter().map(|s| StandardChoice { key: s.key.clone(), label: s.name(lang).to_string() }).collect();
        let chosen = library.hatch_standard();
        let standard_name = l.text("Standaard", "Standard");
        let now = chosen.and_then(|s| standards.iter().find(|c| c.key == s.key).cloned());
        let pick = pick_list(standards, now, |c: StandardChoice| MaterialMessage::Standard(c.key)).text_size(12).padding([2, 6]).width(Fill);
        let more_label = l.text("+ Standaard", "+ Standard");
        let more = a11y::named(button(text(more_label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(MaterialMessage::AddStandard), Aria::Button, more_label);
        fields = fields.push(caption(l.text("Arcering volgens standaard", "Hatch by standard"))).push(labelled(p, standard_name, a11y::named(pick, Aria::ComboBox, standard_name))).push(row![Space::new().width(Fill), more]);
        if let Some(standard) = chosen {
            if !given(standard) {
                let name = l.text("Naam van de standaard", "Name of the standard");
                let typed = view.standard.clone().unwrap_or_else(|| standard.name(lang).to_string());
                fields = fields.push(labelled(p, name, a11y::named(text_input("", &typed).on_input(MaterialMessage::StandardNamed).size(12).padding([2, 6]), Aria::TextField, name)));
            }
            let own = standard.of(m);
            let choices = |none: &'static str| -> Vec<SignChoice> {
                std::iter::once(SignChoice { key: None, label: none.to_string() })
                    .chain(bim_library::hatch::catalogue().iter().map(|h| SignChoice { key: Some(h.key.clone()), label: format!("{} · {} · {}", h.set.name(), h.sign, h.name(lang)) }))
                    .collect()
            };
            let cut_name = l.text("Doorsnede", "Section");
            let cuts = choices(l.text("volgens soort", "by its kind"));
            let now = cuts.iter().find(|c| c.key == own.cut).cloned();
            let pick = pick_list(cuts, now, |c: SignChoice| MaterialMessage::Cut(c.key)).text_size(12).padding([2, 6]).width(Fill);
            fields = fields.push(labelled(p, cut_name, a11y::named(pick, Aria::ComboBox, cut_name)));
            if let Some(hatch) = own.cut_hatch() {
                fields = fields.push(super::libraries::tile_hatch_look(hatch, p, 56.0, super::libraries::HATCH_LOOK_SCALE));
            }
            let face_name = l.text("Aanzicht", "Face seen");
            let faces = choices(l.text("geen arcering", "no hatch"));
            let now = faces.iter().find(|c| c.key == own.face).cloned();
            let pick = pick_list(faces, now, |c: SignChoice| MaterialMessage::Face(c.key)).text_size(12).padding([2, 6]).width(Fill);
            fields = fields.push(labelled(p, face_name, a11y::named(pick, Aria::ComboBox, face_name)));
            if let Some(hatch) = own.face_hatch() {
                fields = fields.push(super::libraries::tile_hatch_look(hatch, p, 56.0, super::libraries::HATCH_LOOK_SCALE));
            }
            let from_name = l.text("Arceren vanaf schaal 1 :", "Hatched from scale 1 :");
            let typed = view.from.clone().unwrap_or_else(|| if own.from > 0.0 { own.from.to_string() } else { String::new() });
            let always = l.text("altijd", "always");
            fields = fields.push(labelled(p, from_name, a11y::named(text_input(always, &typed).on_input(MaterialMessage::From).size(12).padding([2, 6]), Aria::TextField, from_name)));
            let below_name = l.text("Op een kleinere tekening", "On a smaller drawing");
            let below = row([(Below::Fill, l.text("vulling", "fill")), (Below::Nothing, l.text("niets", "nothing"))].map(|(below, label)| {
                let on = own.below == below;
                a11y::named(button(text(label).size(12).color(if on { p.text } else { p.text_secondary })).padding([3, 8]).style(move |_, s| if on { p.ribbon_button(true, s) } else { p.row_button(s) }).on_press(MaterialMessage::Below(below)), Aria::Button, label)
            }))
            .spacing(2);
            fields = fields.push(labelled(p, below_name, below.into()));
            if m.hatches.contains_key(&standard.key) {
                let usual = l.text("Volgens het teken van het materiaal", "By the material's sign");
                fields = fields.push(a11y::named(button(text(usual).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(MaterialMessage::Usual), Aria::Button, usual));
            }
            fields = fields.push(text(l.text("Een aanzicht wordt nog niet gearceerd op een tekening.", "A face seen is not hatched on a drawing yet.")).size(11).color(p.text_muted).wrapping(Wrapping::Word));
        }
        // what is made of it
        let [buildups, linear, systems] = users(library, &m.key);
        let made: Vec<String> = [(l.text("Opbouwen", "Build-ups"), buildups), (l.text("Staaftypen", "Linear types"), linear), (l.text("Systemen", "Systems"), systems)]
            .into_iter()
            .filter(|(_, count)| *count > 0)
            .map(|(what, count)| format!("{what} {count}"))
            .collect();
        let made = if made.is_empty() { l.text("geen type", "no type").to_string() } else { made.join(" · ") };
        fields = fields.push(labelled(p, l.text("Gebruikt door", "Used by"), text(made).size(12).color(p.text_secondary).wrapping(Wrapping::Word).into()));
        let act = |label: &'static str, message: MaterialMessage| button(text(label).size(12).color(p.text_secondary)).padding([3, 8]).style(move |_, s| p.row_button(s)).on_press(message);
        let held = view.in_use.map(|used| {
            let held = l.text("Niet verwijderd: het materiaal is nog in gebruik. Aantal types:", "Not removed: the material is still in use. Number of types:");
            text(format!("{held} {used}")).size(11).color(p.accent).wrapping(Wrapping::Word)
        });
        let editor = fields.push(row![act(l.text("Dupliceren", "Duplicate"), MaterialMessage::Duplicate), act(l.text("Verwijderen", "Remove"), MaterialMessage::Remove), Space::new().width(Fill)].spacing(4)).push(held);
        let pane = column![
            text(m.name(lang)).size(14).font(FONT_BOLD).color(p.text).wrapping(Wrapping::Word),
            text(format!("IfcMaterial · {}", m.key)).size(11).color(p.text_muted),
            looks,
            Element::from(editor).map(Message::Material),
        ]
        .spacing(8);
        container(pane).padding(8).into()
    }
}

#[cfg(test)]
mod tests {
    use super::super::libraries::{LibrariesMessage, Source};
    use super::*;

    #[test]
    fn a_material_takes_a_hatch_of_its_own_from_the_catalogue() {
        let mut app = App::default();
        let _ = app.material_update(MaterialMessage::Pick("timber".into()));
        assert_eq!(app.library.material("timber").unwrap().sign.as_deref(), Some("openaec-12"), "timber starts with the softwood sign");
        let _ = app.material_update(MaterialMessage::Sign(Some("openaec-12-oak".into())));
        assert_eq!(app.library.material("timber").unwrap().catalogue_hatch().map(|h| h.name(Lang::En)), Some("oak"));
        // what the catalogue lacks is not taken; none: drawn by its hatch kind again
        let _ = app.material_update(MaterialMessage::Sign(Some("openaec-99".into())));
        assert_eq!(app.library.material("timber").unwrap().sign, None);
        let _ = app.material_update(MaterialMessage::Sign(Some("openaec-12-oak".into())));
        let _ = app.material_update(MaterialMessage::Sign(None));
        assert_eq!(app.library.material("timber").unwrap().sign, None);
        // the pane offers the hatches of the catalogue
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1100.0), app.material_pane(app.theme.palette(), Language::English));
        assert!(pane.find("Hatch of its own").is_ok());
    }

    #[test]
    fn a_material_is_hatched_by_the_chosen_standard() {
        use bim_library::MaterialHatch;
        let mut app = App::default();
        let _ = app.material_update(MaterialMessage::Pick("timber".into()));
        let pattern = |app: &App| crate::hatching::cut_pattern(&app.library, app.library.material("timber").unwrap()).expect("timber has a hatch");
        let coloured = pattern(&app);
        assert_eq!(coloured.fill, bim_library::hatch::hatch("openaec-12").unwrap().fill, "the suite's own standard is the chosen one");
        // NEN 47 chosen: the same sign, in black on the paper
        let _ = app.material_update(MaterialMessage::Standard("nen47".into()));
        assert_eq!(app.library.hatch_standard().map(|s| s.key.as_str()), Some("nen47"));
        let black = pattern(&app);
        assert_eq!((black.fill, black.grids.clone()), (None, coloured.grids.clone()));
        assert!(app.library.material("timber").unwrap().hatches.is_empty(), "choosing a standard names nothing");
        // its own hatch under it, drawn from 1 : 100, and nothing on a smaller drawing
        let _ = app.material_update(MaterialMessage::Cut(Some("nen47-13".into())));
        let _ = app.material_update(MaterialMessage::Face(Some("nen47-14".into())));
        let _ = app.material_update(MaterialMessage::From("1 : 100".into()));
        assert_eq!(app.library.material_hatch("timber"), Some(MaterialHatch { cut: Some("nen47-13".into()), face: Some("nen47-14".into()), from: 100.0, below: Below::Fill }));
        let own = pattern(&app);
        assert_eq!((own.from, own.below), (100.0, Some(app.library.material("timber").unwrap().colour)), "a hatch without a colour leaves the material's");
        assert_eq!((own.clone().at(100.0), own.clone().at(200.0).grids.len(), own.clone().at(200.0).fill), (own.clone(), 0, own.below));
        let _ = app.material_update(MaterialMessage::Below(Below::Nothing));
        assert_eq!(pattern(&app).at(200.0).look().0, [255; 3], "the paper");
        // what is no scale or no hatch is not taken; what was typed stays in its field
        let _ = app.material_update(MaterialMessage::From("often".into()));
        let _ = app.material_update(MaterialMessage::Cut(Some("nen47-999".into())));
        assert_eq!((app.library.material_hatch("timber").map(|h| (h.cut, h.from)), app.material_view.from.as_deref()), (Some((Some("nen47-13".into()), 100.0)), Some("often")));
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1500.0), app.material_pane(app.theme.palette(), Language::English));
            assert!(pane.find("Hatch by standard").is_ok() && pane.find("Face seen").is_ok() && pane.find("Hatched from scale 1 :").is_ok() && pane.find("On a smaller drawing").is_ok() && pane.find("By the material's sign").is_ok());
            assert!(pane.find("Name of the standard").is_err(), "NEN 47 keeps its name");
        }
        // the other standard still has what the sign gives
        let _ = app.material_update(MaterialMessage::Standard("openaec".into()));
        assert_eq!((pattern(&app), app.material_view.from.clone()), (coloured.clone(), None));
        // a standard of the project: a copy of the chosen one, chosen and named
        let _ = app.material_update(MaterialMessage::Standard("nen47".into()));
        let _ = app.material_update(MaterialMessage::AddStandard);
        assert_eq!(app.library.hatch_standards.iter().map(|s| (s.key.as_str(), s.chosen)).collect::<Vec<_>>(), [("openaec", false), ("nen47", false), ("own-standard", true)]);
        assert_eq!(app.library.material_hatch("timber").and_then(|h| h.cut).as_deref(), Some("nen47-13"), "what the materials name comes along");
        let _ = app.material_update(MaterialMessage::StandardNamed("Bureau ".into()));
        assert_eq!(app.library.hatch_standard().map(|s| s.name(Lang::Nl)), Some("Bureau"));
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1500.0), app.material_pane(app.theme.palette(), Language::English));
            assert!(pane.find("Name of the standard").is_ok());
        }
        // by the material's sign again under the project's standard; NEN 47 keeps its own
        let _ = app.material_update(MaterialMessage::Usual);
        assert_eq!(app.library.material_hatch("timber"), Some(MaterialHatch { cut: Some("nen47-12".into()), ..MaterialHatch::default() }));
        let _ = app.material_update(MaterialMessage::Standard("nen47".into()));
        let _ = app.material_update(MaterialMessage::StandardNamed("Other".into()));
        assert_eq!((app.library.material_hatch("timber").and_then(|h| h.cut).as_deref(), app.library.hatch_standard().map(|s| s.name(Lang::En))), (Some("nen47-13"), Some("NEN 47")));
        // the model's parts take the hatch of the chosen standard
        let parts = crate::hatching::part_materials(&app.library);
        let timber = parts.iter().find(|part| part.name == "timber").expect("timber");
        assert_eq!(timber.pattern.as_ref().map(|p| p.from), Some(100.0));
    }

    #[test]
    fn a_scale_is_typed_with_or_without_its_one() {
        assert_eq!((scale_of("100"), scale_of("1:50"), scale_of(" 1 : 200 "), scale_of(""), scale_of("12,5")), (Some(100.0), Some(50.0), Some(200.0), Some(0.0), Some(12.5)));
        assert_eq!((scale_of("-5"), scale_of("x"), scale_of("1:")), (None, None, None));
    }

    #[test]
    fn a_material_has_a_grade_and_a_timber_class_says_what_it_bears() {
        let mut app = App::default();
        let _ = app.material_update(MaterialMessage::Pick("timber".into()));
        let _ = app.material_update(MaterialMessage::Graded(" C24 ".into()));
        assert_eq!(app.library.material("timber").unwrap().grade.as_deref(), Some("C24"));
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 1200.0), app.material_pane(app.theme.palette(), Language::English));
            assert!(pane.find("Grade").is_ok() && pane.find("fm,k 24 · ft,0,k 14 · fc,0,k 21 · fv,k 4 N/mm²").is_ok() && pane.find("E0,mean 11000 · Gmean 690 N/mm² · ρk 350 kg/m³").is_ok());
        }
        // a grade that is no timber class is kept as it is typed; an empty one is none
        let _ = app.material_update(MaterialMessage::Graded("S355".into()));
        assert_eq!((app.library.material("timber").unwrap().grade.as_deref(), app.library.material("timber").unwrap().timber_grade()), (Some("S355"), None));
        let _ = app.material_update(MaterialMessage::Graded("  ".into()));
        assert_eq!(app.library.material("timber").unwrap().grade, None);
    }

    #[test]
    fn the_dutch_library_has_the_usual_layered_build_ups() {
        let app = App::default();
        let dutch = app.shelf(Shelf::Netherlands);
        let cavity = dutch.buildup("nl.wall.cavity-mineral-wool-sand-lime-100").expect("a cavity wall");
        assert_eq!((cavity.layers.len(), cavity.nlsfb(&dutch.materials).as_str()), (4, "21"));
        assert_eq!(dutch.buildups.iter().filter(|b| b.id.starts_with("nl.")).count(), bim_library::dutch::buildups().len());
    }

    #[test]
    fn the_dutch_library_has_the_materials_of_ocondat() {
        let mut app = App::default();
        let _ = app.libraries_update(LibrariesMessage::Country(super::super::libraries::Country::Netherlands));
        let dutch = app.shelf(Shelf::Netherlands).clone();
        for key in ["spruce", "bluestone", "steel-s355", "concrete-c20-25", "stone-wool", "reed"] {
            assert!(dutch.material(key).is_some(), "{key}");
        }
        assert!(dutch.materials.len() > 100);
        // one of them is edited as a copy in the project, with its hatch
        let _ = app.material_update(MaterialMessage::Copy(Shelf::Netherlands, "bluestone".into()));
        assert_eq!(app.library.material("bluestone").and_then(|m| m.sign.as_deref()), Some("openaec-9-bluestone"));
    }

    #[test]
    fn a_colour_is_typed_as_six_hexadecimal_digits() {
        assert_eq!(hex([185, 185, 185]), "#b9b9b9");
        assert_eq!((colour_of("#A0522D"), colour_of(" a0522d ")), (Some([160, 82, 45]), Some([160, 82, 45])));
        // not while it is being typed, and not what is no colour
        assert_eq!((colour_of("#a05"), colour_of("#a0522dff"), colour_of("#a0522g"), colour_of("")), (None, None, None, None));
    }

    #[test]
    fn a_material_is_named_coloured_and_hatched() {
        let mut app = App::default();
        let materials = app.library.materials.len();
        // a material more: grey, the general hatch, picked to be edited
        let _ = app.material_update(MaterialMessage::Add);
        let key = app.material_view.picked.clone().expect("the new material is picked");
        let material = |app: &App| app.library.material(&key).unwrap().clone();
        assert_eq!((app.library.materials.len(), key.as_str(), material(&app).hatch), (materials + 1, "own-material-1", HatchKind::Generic));
        // its name in a language, as it is typed; a name cannot be empty, its field can
        let _ = app.material_update(MaterialMessage::Named(Lang::Nl, "leem ".into()));
        assert_eq!((material(&app).name(Lang::Nl), material(&app).name(Lang::En)), ("leem", "new material"));
        assert_eq!(app.material_view.name, Some((Lang::Nl, "leem ".to_string())), "what was typed stays in its field");
        let _ = app.material_update(MaterialMessage::Named(Lang::En, "loam".into()));
        let _ = app.material_update(MaterialMessage::Named(Lang::En, String::new()));
        assert_eq!((material(&app).name(Lang::En), app.material_view.name.clone()), ("loam", Some((Lang::En, String::new()))));
        // its colour: typed in full, or a channel at a time
        let _ = app.material_update(MaterialMessage::Hex("#a0522d".into()));
        assert_eq!(material(&app).colour, [160, 82, 45]);
        let _ = app.material_update(MaterialMessage::Hex("#a05".into()));
        assert_eq!((material(&app).colour, app.material_view.hex.as_deref()), ([160, 82, 45], Some("#a05")), "not before it is a colour");
        let _ = app.material_update(MaterialMessage::Channel(2, 0));
        let _ = app.material_update(MaterialMessage::Shown);
        assert_eq!((material(&app).colour, app.material_view.hex.clone()), ([160, 82, 0], None));
        // its hatch, and its key as it was
        let _ = app.material_update(MaterialMessage::Hatch(HatchKind::Brick));
        assert_eq!((material(&app).hatch, material(&app).key), (HatchKind::Brick, key.clone()));
        // a copy to change, told apart by its names, and gone again
        let _ = app.material_update(MaterialMessage::Duplicate);
        let copy = app.material_view.picked.clone().unwrap();
        assert_ne!(copy, key);
        assert_eq!((app.library.material(&copy).unwrap().name(Lang::Nl), app.library.material(&copy).unwrap().colour), ("leem 2", [160, 82, 0]));
        let _ = app.material_update(MaterialMessage::Remove);
        assert!(app.library.material(&copy).is_none() && app.material_view.picked.is_none() && app.library.material(&key).is_some());
    }

    #[test]
    fn a_material_a_type_is_made_of_is_not_removed() {
        let mut app = App::default();
        // concrete: build-ups and columns are made of it
        let [buildups, linear, _] = users(&app.library, "concrete");
        assert!(buildups > 0 && linear > 0);
        let _ = app.material_update(MaterialMessage::Pick("concrete".into()));
        let _ = app.material_update(MaterialMessage::Remove);
        assert!(app.library.material("concrete").is_some());
        assert_eq!(app.material_view.in_use, Some(users(&app.library, "concrete").iter().sum()));
        // every type still checks, and a change takes the remark away
        assert!(app.library.buildups.iter().all(|b| b.check(&app.library.materials).is_ok()));
        let _ = app.material_update(MaterialMessage::Hatch(HatchKind::Concrete));
        assert_eq!(app.material_view.in_use, None);
        // one no type is made of goes
        let _ = app.material_update(MaterialMessage::Add);
        let key = app.material_view.picked.clone().unwrap();
        assert_eq!(users(&app.library, &key), [0, 0, 0]);
        let _ = app.material_update(MaterialMessage::Remove);
        assert!(app.library.material(&key).is_none());
    }

    #[test]
    fn a_material_of_another_library_is_edited_as_a_copy_under_its_key() {
        let mut app = App::default();
        // one the project has not got: it comes under the key it had
        let other = Material::new("rammed-earth", Names::same("stampleem"), HatchKind::Generic, [150, 110, 80]);
        let mut from = Library::default();
        from.add_material(other.clone()).unwrap();
        assert_eq!(copy(&mut app.library, &from, "rammed-earth").as_deref(), Ok("rammed-earth"));
        assert_eq!(app.library.material("rammed-earth"), Some(&other));
        // one the project has: its own is the one edited, and there is no second
        let materials = app.library.materials.len();
        let _ = app.libraries_update(LibrariesMessage::Search("concrete".into()));
        let _ = app.material_update(MaterialMessage::Copy(Shelf::General, "concrete".into()));
        assert_eq!((app.library.materials.len(), app.material_view.picked.as_deref()), (materials, Some("concrete")));
        assert_eq!((app.libraries.source, app.libraries.query.as_str()), (Source::Project, ""));
        // the right pane edits what was picked last: a material, a build-up or a linear type
        let wall = app.library.buildups[0].id.clone();
        let _ = app.buildup_update(super::super::buildups::BuildUpMessage::Pick(wall.clone()));
        assert_eq!((app.material_view.picked.clone(), app.buildup_view.picked.clone()), (None, Some(wall)));
        let _ = app.material_update(MaterialMessage::Pick("steel".into()));
        assert_eq!((app.material_view.picked.as_deref(), app.buildup_view.picked.clone(), app.profile_view.picked.clone()), (Some("steel"), None, None));
        let column = app.library.linear_types[0].id.clone();
        let _ = app.profile_update(super::super::profiles::ProfileMessage::Pick(column.clone()));
        assert_eq!((app.material_view.picked.clone(), app.profile_view.picked.clone()), (None, Some(column)));
    }

    #[test]
    fn the_project_page_shows_the_materials_and_the_pane_edits_the_picked_one() {
        let mut app = App::default();
        let l = Language::English;
        let _ = app.libraries_update(LibrariesMessage::Source(Source::Project));
        {
            // the project's first category: its materials, a tile each, and one to make another
            let mut page = crate::app::test_ui_of(iced::Size::new(1200.0, 1400.0), app.libraries_page(app.theme.palette(), l));
            assert!(page.find("Materials").is_ok() && page.find("+ Material").is_ok() && page.find("brickwork").is_ok() && page.find("PIR insulation").is_ok());
        }
        {
            let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 800.0), app.material_pane(app.theme.palette(), l));
            assert!(pane.find("Pick a material on the page to edit it.").is_ok());
        }
        let _ = app.material_update(MaterialMessage::Pick("brick".into()));
        let mut pane = crate::app::test_ui_of(iced::Size::new(340.0, 900.0), app.material_pane(app.theme.palette(), l));
        assert!(pane.find("IfcMaterial · brick").is_ok() && pane.find("Nederlands").is_ok() && pane.find("Français").is_ok());
        // the hatches to pick from, named after the general library's materials, and what is
        // made of it
        assert!(pane.find("sand-lime brick").is_ok() && pane.find("air cavity").is_ok() && pane.find("Used by").is_ok() && pane.find("Duplicate").is_ok());
    }
}
