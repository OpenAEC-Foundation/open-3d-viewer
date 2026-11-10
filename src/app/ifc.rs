//! The IFC tab: how the IFCX file of the model is built up, under the 3D view as a schedule is.
//! It reads the IFCX file the model was opened from, else the IFCX that Save as writes for it
//! (the model and its document), and shows it four ways:
//! - **Text**: every member on its own line in the file's own order, lists of numbers on one line;
//! - **Nodes**: the tree from the roots along children and inherits, opened where asked;
//! - **Table**: the `data` entries as rows, in file order;
//! - **Relations**: the links between nodes, named after the IFC relation they stand for.
//!
//! A node picked in any of them selects its elements in the model (a storey: all in it); an
//! element selected in the model picks its node, opens the tree down to it and scrolls it into
//! sight. Only the rows in sight are drawn, so a file of a hundred thousand lines scrolls as a
//! short one.

use super::{App, Message, FONT_BOLD};
use crate::ifcx_structure::{Relation, Structure, TreeRow, Via};
use crate::selection::Pickable;
use crate::theme::Palette;
use iced::widget::operation::AbsoluteOffset;
use iced::widget::scrollable::{Direction, Scrollbar, Viewport};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, operation, row, rule, scrollable, text, text_input, Column, Space};
use iced::{Center, Color, Element, Fill, Font, Task};
use openaec_ui::Language;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Height of a line of text or a row (logical pixels).
const LINE: f32 = 20.0;
/// Width of a character of the monospaced font at size 12 (logical pixels).
const CHAR: f32 = 7.3;
/// Indentation of a tree level (logical pixels).
const STEP: f32 = 16.0;
/// Elements a picked node selects at most (a storey of a large model).
const SELECT_LIMIT: usize = 100_000;
/// The scrollable of the table's headings.
const TABLE_HEAD: &str = "ifc-table-head";
/// Nodes found whose place in the tree is opened, at most.
const MATCHES_OPENED: usize = 5_000;

/// What the IFC tab shows of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IfcView {
    #[default]
    Text,
    Nodes,
    Table,
    Relations,
}

impl IfcView {
    const ALL: [IfcView; 4] = [IfcView::Text, IfcView::Nodes, IfcView::Table, IfcView::Relations];

    fn label(self, l: Language) -> &'static str {
        match self {
            IfcView::Text => l.text("Tekst", "Text"),
            IfcView::Nodes => l.text("Nodes", "Nodes"),
            IfcView::Table => l.text("Tabel", "Table"),
            IfcView::Relations => l.text("Relaties", "Relations"),
        }
    }

    /// Its scrollable.
    fn id(self) -> &'static str {
        match self {
            IfcView::Text => "ifc-text",
            IfcView::Nodes => "ifc-nodes",
            IfcView::Table => "ifc-table",
            IfcView::Relations => "ifc-relations",
        }
    }
}

/// The IFC tab's state.
#[derive(Debug, Clone, Default)]
pub struct IfcTab {
    /// Its tab is open, and shown (under the 3D view).
    pub open: bool,
    pub shown: bool,
    /// The file taken apart.
    pub structure: Option<Arc<Structure>>,
    pub view: IfcView,
    /// Per view: scrolled offset and height (logical pixels).
    pub scroll: [(f32, f32); 4],
    /// The model's elements of the nodes, and the node of each element.
    pub elements: HashMap<u32, Pickable>,
    pub node_of: HashMap<u64, u32>,
    /// The picked node, and the element selected in the model it last followed.
    pub pick: Option<u32>,
    pub followed: Option<u64>,
    /// The opened rows of the tree, and its rows as they show.
    pub opened: HashSet<String>,
    pub rows: Vec<TreeRow>,
    /// The relation the Relations view shows (all when `None`), and those links.
    pub relation: Option<Relation>,
    pub links: Vec<u32>,
    /// What is searched for (any case); the lines of the text holding it and the one jumped to;
    /// the nodes whose path, name or class holds it; and the entries the table shows.
    pub query: String,
    pub hits: Vec<u32>,
    pub hit: usize,
    pub matches: HashSet<u32>,
    pub table: Vec<u32>,
}

impl IfcTab {
    fn scroll(&self, view: IfcView) -> (f32, f32) {
        self.scroll[view as usize]
    }
}

impl App {
    /// Opens the IFC tab, or shows it: the file is read again, so it follows the model and its
    /// document.
    pub(super) fn open_ifc(&mut self) -> Task<Message> {
        let s = match self.ifcx_source().and_then(|(name, bytes, links)| Ok((Structure::read(&name, &bytes)?, links))) {
            Ok(s) => s,
            Err(e) => {
                self.status = super::Status::Error(e);
                return Task::none();
            }
        };
        let (s, links) = s;
        let previous = std::mem::take(&mut self.ifc);
        self.ifc = IfcTab { open: true, view: previous.view, relation: previous.relation, ..IfcTab::default() };
        // the model's elements by node: the file's own paths (`Id`), or those Save as gives them
        for (path, item) in links {
            if let Some(&k) = s.index.get(&path) {
                self.ifc.elements.insert(k, item);
                if let Pickable::Element(id) = item {
                    self.ifc.node_of.insert(id, k);
                }
            }
        }
        // the roots and what they hold open
        for &r in &s.roots {
            let root = s.nodes[r as usize].path.clone();
            for (name, _) in &s.nodes[r as usize].children {
                self.ifc.opened.insert(format!("{root}{}{name}", crate::ifcx_structure::KEY_SEP));
            }
            self.ifc.opened.insert(root);
        }
        self.ifc.structure = Some(Arc::new(s));
        self.ifc_filter();
        self.show_ifc();
        let follow = self.ifc_follow();
        Task::batch([operation::snap_to(self.ifc.view.id(), iced::widget::scrollable::RelativeOffset::START), follow])
    }

    /// Shows the IFC tab again, as it was.
    pub(super) fn show_ifc(&mut self) {
        self.active_schedule = None;
        self.active_sheet = None;
        self.active_doc_sheet = None;
        self.load_view(None);
        self.ifc.shown = true;
    }

    /// Closes the IFC tab.
    pub(super) fn close_ifc(&mut self) {
        let view = self.ifc.view;
        self.ifc = IfcTab { view, ..IfcTab::default() };
    }

    /// The IFCX the tab shows: the file the model was opened from (desktop), else what Save as
    /// writes for the model and its document; with the node path of each of the model's elements.
    fn ifcx_source(&self) -> Result<(String, Vec<u8>, Vec<(String, Pickable)>), String> {
        let model = self.model.as_ref().ok_or("no model is open")?;
        #[cfg(not(target_arch = "wasm32"))]
        if self.models.len() <= 1 && model.path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ifcx")) {
            if let Ok(bytes) = std::fs::read(&model.path) {
                // the reader keeps the path of an element's node as its `Id`
                let links = model
                    .props
                    .iter()
                    .filter_map(|(&id, json)| {
                        let v: serde_json::Value = serde_json::from_str(json).ok()?;
                        Some((v.get("Id")?.as_str()?.to_string(), Pickable::Element(id)))
                    })
                    .collect();
                return Ok((model.file_name(), bytes, links));
            }
        }
        let stem = model.path.file_stem().map_or_else(|| "model".to_string(), |s| s.to_string_lossy().into_owned());
        let file = format!("{stem}.ifcx");
        let bytes = crate::export::to_bytes(model, &self.document_now(), &file)?;
        let mut links = Vec::new();
        for (li, layer) in model.layers.iter().enumerate().filter(|(_, l)| !l.points) {
            if layer.elements.is_empty() {
                links.push((crate::export::ifcx::element_path(&file, li, u64::MAX), Pickable::Layer(li)));
            }
            for &(id, _, _) in &layer.elements {
                links.push((crate::export::ifcx::element_path(&file, li, id), Pickable::Element(id)));
            }
        }
        Ok((file, bytes, links))
    }

    /// The rows of the tree as it is opened; while searching, the nodes found with the rows above
    /// them only (opened down to them).
    fn ifc_rows(&mut self) {
        let Some(s) = self.ifc.structure.clone() else { return };
        if self.ifc.query.is_empty() {
            self.ifc.rows = s.tree_rows(&self.ifc.opened);
            return;
        }
        let above: HashSet<String> = self.ifc.matches.iter().take(MATCHES_OPENED).flat_map(|&k| s.keys_above(k)).collect();
        let opened: HashSet<String> = self.ifc.opened.union(&above).cloned().collect();
        let matches = &self.ifc.matches;
        self.ifc.rows = s.tree_rows(&opened).into_iter().filter(|r| r.node.is_some_and(|k| matches.contains(&k)) || above.contains(&r.key)).collect();
    }

    /// The links the Relations view shows: of the relation asked for, and while searching those
    /// from or to a node found, or named after what is searched for.
    fn ifc_links(&mut self) {
        let Some(s) = &self.ifc.structure else { return };
        let want = self.ifc.relation;
        let query = self.ifc.query.to_lowercase();
        let matches = &self.ifc.matches;
        self.ifc.links = (0..s.links.len() as u32)
            .filter(|&k| {
                let link = &s.links[k as usize];
                want.is_none_or(|r| link.kind == r)
                    && (query.is_empty() || matches.contains(&link.from) || link.to.is_some_and(|t| matches.contains(&t)) || link.name.to_lowercase().contains(&query))
            })
            .collect();
    }

    /// What the search finds: the lines, nodes, entries, tree rows and links.
    fn ifc_filter(&mut self) {
        let Some(s) = self.ifc.structure.clone() else { return };
        let query = self.ifc.query.to_lowercase();
        let holds = |t: &str| t.to_lowercase().contains(&query);
        if query.is_empty() {
            self.ifc.hits.clear();
            self.ifc.matches.clear();
            self.ifc.table = (0..s.entries.len() as u32).collect();
        } else {
            self.ifc.hits = (0..s.lines.len() as u32).filter(|&k| holds(&s.lines[k as usize].text)).collect();
            self.ifc.matches = (0..s.nodes.len() as u32)
                .filter(|&k| {
                    let n = &s.nodes[k as usize];
                    holds(&n.path) || n.name.as_deref().is_some_and(holds) || n.class.as_deref().is_some_and(holds)
                })
                .collect();
            self.ifc.table = (0..s.entries.len() as u32)
                .filter(|&k| {
                    let e = &s.entries[k as usize];
                    holds(&e.path) || e.name.as_deref().is_some_and(holds) || e.class.as_deref().is_some_and(holds) || e.attributes.iter().any(|(a, _)| holds(a))
                })
                .collect();
        }
        self.ifc.hit = 0;
        self.ifc_rows();
        self.ifc_links();
    }

    /// Searches the file: every view shows what holds `query`, the text jumps to the first line.
    pub(super) fn ifc_query(&mut self, query: String) -> Task<Message> {
        self.ifc.query = query;
        self.ifc_filter();
        let jump = self.ifc_jump();
        let tops = [IfcView::Nodes, IfcView::Table, IfcView::Relations].map(|v| operation::snap_to(v.id(), iced::widget::scrollable::RelativeOffset::START));
        Task::batch(tops.into_iter().chain([jump]))
    }

    /// The next (or the previous) line found in the text, round the end.
    pub(super) fn ifc_next(&mut self, forward: bool) -> Task<Message> {
        let n = self.ifc.hits.len();
        if n == 0 {
            return Task::none();
        }
        self.ifc.hit = if forward { (self.ifc.hit + 1) % n } else { (self.ifc.hit + n - 1) % n };
        self.ifc_jump()
    }

    /// Scrolls the text to the line found that is jumped to.
    fn ifc_jump(&self) -> Task<Message> {
        let Some(&line) = self.ifc.hits.get(self.ifc.hit) else { return Task::none() };
        let (_, height) = self.ifc.scroll(IfcView::Text);
        let top = (line as f32 * LINE - height.max(LINE * 6.0) / 3.0).max(0.0);
        operation::scroll_to(IfcView::Text.id(), AbsoluteOffset { x: None, y: Some(top) })
    }

    /// Shows another view of the file, scrolled to the picked node.
    pub(super) fn ifc_show_view(&mut self, view: IfcView) -> Task<Message> {
        self.ifc.view = view;
        self.ifc_scroll_to_pick(true)
    }

    /// Opens or closes a row of the tree.
    pub(super) fn ifc_toggle(&mut self, key: String) {
        if !self.ifc.opened.remove(&key) {
            self.ifc.opened.insert(key);
        }
        self.ifc_rows();
    }

    /// The Relations view shows one relation, or all.
    pub(super) fn ifc_relation(&mut self, relation: Option<Relation>) -> Task<Message> {
        self.ifc.relation = relation;
        self.ifc_links();
        operation::snap_to(IfcView::Relations.id(), iced::widget::scrollable::RelativeOffset::START)
    }

    /// A node picked in the tab: its elements (and those of what it holds) are selected in the
    /// model; a node without any (a body) selects the element it is part of.
    pub(super) fn ifc_pick(&mut self, node: u32) {
        let Some(s) = self.ifc.structure.clone() else { return };
        self.ifc.pick = Some(node);
        let mut seen = HashSet::new();
        let mut items: Vec<Pickable> = s.below(node, SELECT_LIMIT).iter().filter_map(|k| self.ifc.elements.get(k).copied()).filter(|p| seen.insert(*p)).collect();
        if items.is_empty() {
            items = s.nodes_above(node).iter().rev().find_map(|k| self.ifc.elements.get(k).copied()).into_iter().collect();
        }
        if items.is_empty() {
            return;
        }
        self.set_structural_selection(Vec::new());
        self.sel.clear();
        self.sel.extend(items);
        self.selection_changed();
        // the tab does not move to the selection it made
        self.ifc.followed = self.selected.and_then(|(_, id)| id);
    }

    /// The element selected in the model: its node is picked, the tree opened down to it, and
    /// the shown view scrolled to it when it is out of sight.
    pub(super) fn ifc_follow(&mut self) -> Task<Message> {
        let Some(s) = self.ifc.structure.clone() else { return Task::none() };
        let id = self.selected.and_then(|(_, id)| id);
        if id == self.ifc.followed {
            return Task::none();
        }
        self.ifc.followed = id;
        let Some(&node) = id.and_then(|id| self.ifc.node_of.get(&id)) else { return Task::none() };
        if self.ifc.pick == Some(node) {
            return Task::none();
        }
        self.ifc.pick = Some(node);
        let above = s.keys_above(node);
        if above.iter().any(|k| !self.ifc.opened.contains(k)) {
            self.ifc.opened.extend(above);
            self.ifc_rows();
        }
        self.ifc_scroll_to_pick(false)
    }

    /// Scrolls the shown view to the picked node (always, or only when it is out of sight).
    fn ifc_scroll_to_pick(&self, always: bool) -> Task<Message> {
        let (Some(s), Some(node)) = (&self.ifc.structure, self.ifc.pick) else { return Task::none() };
        let n = &s.nodes[node as usize];
        let view = self.ifc.view;
        let at = match view {
            IfcView::Text => n.entries.first().map(|&e| s.entries[e as usize].line as usize),
            IfcView::Nodes => {
                let key = s.keys.get(&node);
                self.ifc.rows.iter().position(|r| r.node == Some(node) && Some(&r.key) == key).or_else(|| self.ifc.rows.iter().position(|r| r.node == Some(node)))
            }
            IfcView::Table => self.ifc.table.iter().position(|e| n.entries.contains(e)),
            IfcView::Relations => self.ifc.links.iter().position(|&k| s.links[k as usize].from == node || s.links[k as usize].to == Some(node)),
        };
        let Some(at) = at else { return Task::none() };
        let (offset, height) = self.ifc.scroll(view);
        let y = at as f32 * LINE;
        if !always && height > 0.0 && y >= offset && y + LINE <= offset + height {
            return Task::none();
        }
        let top = (y - height.max(LINE * 6.0) / 3.0).max(0.0);
        operation::scroll_to(view.id(), AbsoluteOffset { x: None, y: Some(top) })
    }

    /// A view scrolled: remembers which rows are in sight; the table's headings follow its rows
    /// sideways.
    pub(super) fn ifc_scrolled(&mut self, view: IfcView, v: Viewport) -> Task<Message> {
        self.ifc.scroll[view as usize] = (v.absolute_offset().y, v.bounds().height);
        match view {
            IfcView::Table => operation::scroll_to(TABLE_HEAD, AbsoluteOffset { x: Some(v.absolute_offset().x), y: None }),
            _ => Task::none(),
        }
    }

    /// The panel under the 3D view: a bar with the file and what it holds, then the view of it.
    pub(super) fn ifc_panel(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let Some(s) = self.ifc.structure.as_deref() else { return Space::new().into() };
        let size = if s.bytes >= 1 << 20 { format!("{:.1} MB", s.bytes as f64 / (1 << 20) as f64) } else { format!("{} kB", s.bytes.div_ceil(1024)) };
        let info = format!(
            "{} {} · {} {} · {} {} · {size}",
            s.nodes.len(),
            l.text("nodes", "nodes"),
            s.links.len(),
            l.text("relaties", "relations"),
            s.lines.len(),
            l.text("regels", "lines")
        );
        // the views first, then the file (cut off when the bar is narrow), then the search
        let mut bar = row![text("IFC").size(12).font(FONT_BOLD).color(p.text)].spacing(10).align_y(Center).padding([4, 8]);
        for view in IfcView::ALL {
            let active = self.ifc.view == view;
            bar = bar.push(
                button(text(view.label(l)).size(12).color(if active { p.accent } else { p.text_secondary }))
                    .padding([2, 8])
                    .style(move |_, st| p.ribbon_button(active, st))
                    .on_press(Message::IfcView(view)),
            );
        }
        let file = row![text(s.source.as_str()).size(12).color(p.text).wrapping(Wrapping::None), text(info).size(11).color(p.text_muted).wrapping(Wrapping::None)].spacing(10).align_y(Center);
        bar = bar.push(container(file).width(Fill).clip(true));
        // the search: what it finds in the view shown; the text jumps from line to line
        let search = text_input(l.text("Zoeken in IFC…", "Search IFC…"), &self.ifc.query)
            .on_input(Message::IfcQuery)
            .on_submit(Message::IfcNext(true))
            .size(12)
            .padding([3, 8])
            .width(220);
        bar = bar.push(search);
        if !self.ifc.query.is_empty() {
            let found = match self.ifc.view {
                IfcView::Text if self.ifc.hits.is_empty() => l.text("niets gevonden", "nothing found").to_string(),
                IfcView::Text => format!("{} / {}", self.ifc.hit + 1, self.ifc.hits.len()),
                IfcView::Nodes => self.ifc.matches.len().to_string(),
                IfcView::Table => self.ifc.table.len().to_string(),
                IfcView::Relations => self.ifc.links.len().to_string(),
            };
            bar = bar.push(text(found).size(11).color(p.text_muted));
            if self.ifc.view == IfcView::Text && !self.ifc.hits.is_empty() {
                let step = |label: &'static str, forward: bool| button(text(label).size(12).color(p.text_secondary)).padding([0, 5]).style(move |_, st| p.row_button(st)).on_press(Message::IfcNext(forward));
                bar = bar.push(step("▲", false)).push(step("▼", true));
            }
        }
        let close = button(text("×").size(14).color(p.text_secondary)).padding([0, 6]).style(move |_, st| p.row_button(st)).on_press(Message::CloseIfc);
        bar = bar.push(close);
        let body = match self.ifc.view {
            IfcView::Text => self.ifc_text(p, s),
            IfcView::Nodes => self.ifc_tree(p, l, s),
            IfcView::Table => self.ifc_table(p, l, s),
            IfcView::Relations => self.ifc_relations(p, l, s),
        };
        column![bar, rule::horizontal(1), body].into()
    }

    /// The rows in sight of a view with `count` rows: first and last.
    fn ifc_sight(&self, view: IfcView, count: usize) -> (usize, usize) {
        let (offset, height) = self.ifc.scroll(view);
        let first = ((offset / LINE).floor().max(0.0) as usize).min(count);
        let last = (first + (height.max(1200.0) / LINE).ceil() as usize + 2).min(count);
        (first, last)
    }

    /// A row that picks a node, marked when it is the picked one.
    fn ifc_row<'a>(&self, p: Palette, content: impl Into<Element<'a, Message>>, node: Option<u32>) -> Element<'a, Message> {
        let picked = node.is_some() && node == self.ifc.pick;
        let b = button(container(content).height(LINE).align_y(Center)).padding(0).width(Fill).style(move |_, st| if picked { p.ribbon_button(true, st) } else { p.row_button(st) });
        match node {
            Some(k) => b.on_press(Message::IfcPick(k)).into(),
            None => b.into(),
        }
    }

    /// Rows in a scrollable, with space for those out of sight.
    fn ifc_scrollable<'a>(&self, view: IfcView, rows: Column<'a, Message>, width: Option<f32>) -> Element<'a, Message> {
        let s = match width {
            Some(w) => scrollable(rows.width(w)).direction(Direction::Both { vertical: Scrollbar::new(), horizontal: Scrollbar::new() }),
            None => scrollable(rows.width(Fill)),
        };
        s.id(view.id()).on_scroll(move |v| Message::IfcScroll(view, v)).width(Fill).height(Fill).into()
    }

    /// The text: line numbers, then the lines in sight (a cut line ends in its length); the lines
    /// of the picked node in the accent colour, a click on an entry's line picks its node.
    fn ifc_text<'a>(&self, p: Palette, s: &'a Structure) -> Element<'a, Message> {
        let mono = openaec_ui::fonts::MONO;
        let (first, last) = self.ifc_sight(IfcView::Text, s.lines.len());
        let digits = s.lines.len().max(1).to_string().len();
        let number_width = digits as f32 * CHAR + 16.0;
        let widest = s.lines.iter().map(|l| 2 * l.depth as usize + l.text.len().min(crate::ifcx_structure::LINE_CHARS) + if l.chars > l.text.chars().count() { 12 } else { 0 }).max().unwrap_or(0);
        let width = number_width + widest as f32 * CHAR + 24.0;
        let picked: HashSet<u32> = self.ifc.pick.map(|k| s.nodes[k as usize].entries.iter().copied().collect()).unwrap_or_default();
        let mut lines = column![Space::new().height(first as f32 * LINE)];
        for (k, line) in s.lines[first..last].iter().enumerate() {
            let shown = line.text.chars().count();
            let mut t = format!("{}{}", "  ".repeat(line.depth as usize), line.text);
            if line.chars > shown {
                t.push_str(&format!(" … +{}", line.chars - shown));
            }
            let mine = line.entry.is_some_and(|e| picked.contains(&e));
            let number = text(format!("{:>digits$}", first + k + 1)).size(12).font(mono).color(p.text_muted);
            let content = row![container(number).width(number_width).padding([0, 8]), text(t).size(12).font(mono).color(if mine { p.accent } else { p.text }).wrapping(Wrapping::None)].align_y(Center);
            // a line found is marked, the one jumped to more
            let at = (first + k) as u32;
            let found = self.ifc.hits.binary_search(&at).ok().map(|h| if h == self.ifc.hit { 0.32 } else { 0.14 });
            let content = container(content).width(Fill).style(move |_| container::Style { background: found.map(|a| Color { a, ..p.accent }.into()), ..Default::default() });
            let node = line.entry.and_then(|e| s.index.get(&s.entries[e as usize].path).copied());
            lines = lines.push(self.ifc_row(p, content, node));
        }
        lines = lines.push(Space::new().height((s.lines.len() - last) as f32 * LINE));
        self.ifc_scrollable(IfcView::Text, lines, Some(width))
    }

    /// The tree: per row an arrow that opens it, its name where it hangs, the node's class and
    /// name, what it inherits from, and its attributes.
    fn ifc_tree<'a>(&'a self, p: Palette, l: Language, s: &'a Structure) -> Element<'a, Message> {
        let rows = &self.ifc.rows;
        let (first, last) = self.ifc_sight(IfcView::Nodes, rows.len());
        let mut list = column![Space::new().height(first as f32 * LINE)];
        for r in &rows[first..last] {
            let n = r.node.map(|k| &s.nodes[k as usize]);
            let arrow: Element<'a, Message> = if r.kids && !r.cycle {
                button(text(if r.open { "▾" } else { "▸" }).size(12).color(p.text_secondary)).padding([0, 4]).style(move |_, st| p.row_button(st)).on_press(Message::IfcToggle(r.key.clone())).into()
            } else {
                Space::new().width(18).into()
            };
            let mut line = row![Space::new().width(r.depth as f32 * STEP), arrow].spacing(4).align_y(Center);
            if r.via == Via::Inherits {
                line = line.push(text(l.text("erft", "inherits")).size(11).color(p.text_muted));
            }
            line = line.push(text(r.label.as_str()).size(12).font(if r.via == Via::Root { FONT_BOLD } else { super::FONT }).color(p.text).wrapping(Wrapping::None));
            match n {
                Some(n) => {
                    if let Some(class) = &n.class {
                        line = line.push(text(class.as_str()).size(11).color(p.accent).wrapping(Wrapping::None));
                    }
                    if let Some(name) = n.name.as_deref().filter(|name| *name != r.label) {
                        line = line.push(text(name).size(11).color(p.text_secondary).wrapping(Wrapping::None));
                    }
                    if r.via != Via::Root {
                        line = line.push(text(format!("→ {}", r.target)).size(11).font(Font::MONOSPACE).color(p.text_muted).wrapping(Wrapping::None));
                    }
                    let attrs: Vec<&str> = n.attributes.iter().map(|a| a.as_str()).filter(|a| *a != "bsi::ifc::class" && *a != "bsi::ifc::prop::Name").take(4).collect();
                    if !attrs.is_empty() {
                        let more = n.attributes.len().saturating_sub(2 + attrs.len());
                        let t = if more > 0 { format!("{} +{more}", attrs.join(", ")) } else { attrs.join(", ") };
                        line = line.push(text(t).size(11).color(p.text_muted).wrapping(Wrapping::None));
                    }
                }
                None => line = line.push(text(format!("→ {} ({})", r.target, l.text("niet in het bestand", "not in the file"))).size(11).color(p.text_muted).wrapping(Wrapping::None)),
            }
            if r.cycle {
                line = line.push(text(l.text("(houdt zichzelf)", "(holds itself)")).size(11).color(p.text_muted));
            }
            list = list.push(self.ifc_row(p, line.padding([0, 6]), r.node));
        }
        list = list.push(Space::new().height((rows.len() - last) as f32 * LINE));
        self.ifc_scrollable(IfcView::Nodes, list, None)
    }

    /// The table: an entry per row, in file order.
    fn ifc_table<'a>(&'a self, p: Palette, l: Language, s: &'a Structure) -> Element<'a, Message> {
        const W: [f32; 7] = [56.0, 300.0, 170.0, 220.0, 260.0, 200.0, 360.0];
        let cell = |t: iced::widget::Text<'a>, w: f32| container(t.wrapping(Wrapping::None)).width(w).padding([0, 6]).clip(true);
        let headings = [
            "#",
            l.text("Pad", "Path"),
            l.text("Klasse", "Class"),
            l.text("Naam", "Name"),
            l.text("Kinderen", "Children"),
            l.text("Erft van", "Inherits"),
            l.text("Attributen", "Attributes"),
        ];
        let width: f32 = W.iter().sum();
        let header = row(headings.iter().zip(W).map(|(h, w)| cell(text(*h).size(11).font(FONT_BOLD).color(p.text_secondary), w).into())).height(LINE + 4.0).align_y(Center);
        let shown = &self.ifc.table;
        let (first, last) = self.ifc_sight(IfcView::Table, shown.len());
        let mut rows = column![Space::new().height(first as f32 * LINE)];
        let pairs = |v: &[(String, Option<String>)]| v.iter().map(|(n, t)| format!("{n} → {}", t.as_deref().unwrap_or("null"))).collect::<Vec<_>>().join(", ");
        for &k in &shown[first..last] {
            let e = &s.entries[k as usize];
            let attrs: Vec<&str> = e.attributes.iter().map(|(a, _)| a.as_str()).collect();
            let cells = row![
                cell(text((k + 1).to_string()).size(12).color(p.text_muted), W[0]),
                cell(text(e.path.as_str()).size(12).font(Font::MONOSPACE).color(p.text), W[1]),
                cell(text(e.class.as_deref().unwrap_or("")).size(12).color(p.accent), W[2]),
                cell(text(e.name.as_deref().unwrap_or("")).size(12).color(p.text), W[3]),
                cell(text(pairs(&e.children)).size(12).color(p.text), W[4]),
                cell(text(pairs(&e.inherits)).size(12).color(p.text), W[5]),
                cell(text(attrs.join(", ")).size(12).color(p.text_secondary), W[6]),
            ]
            .align_y(Center);
            rows = rows.push(self.ifc_row(p, cells, s.index.get(&e.path).copied()));
        }
        rows = rows.push(Space::new().height((shown.len() - last) as f32 * LINE));
        // the headings stay in place while the rows scroll, and follow them sideways
        column![
            scrollable(header.width(width)).id(TABLE_HEAD).direction(Direction::Horizontal(Scrollbar::hidden())).width(Fill),
            rule::horizontal(1),
            self.ifc_scrollable(IfcView::Table, rows, Some(width)),
        ]
        .into()
    }

    /// The relations: a filter per relation with its count, then a link per row: the node, the
    /// relation, its name there and the node it points at; either end picks its node.
    fn ifc_relations<'a>(&'a self, p: Palette, l: Language, s: &'a Structure) -> Element<'a, Message> {
        let mut count: HashMap<Relation, usize> = HashMap::new();
        for link in &s.links {
            *count.entry(link.kind).or_default() += 1;
        }
        let chip = |label: String, active: bool, on: Option<Relation>| {
            button(text(label).size(11).color(if active { p.accent } else { p.text_secondary })).padding([1, 6]).style(move |_, st| p.ribbon_button(active, st)).on_press(Message::IfcRelation(on))
        };
        let mut chips = row![chip(format!("{} {}", l.text("Alle", "All"), s.links.len()), self.ifc.relation.is_none(), None)].spacing(4).padding([4, 8]);
        for r in Relation::ALL.into_iter().filter(|r| count.contains_key(r)) {
            chips = chips.push(chip(format!("{} {}", r.ifc(), count[&r]), self.ifc.relation == Some(r), Some(r)));
        }
        let end = |k: Option<u32>, fallback: &'a str| -> Element<'a, Message> {
            let label = k.map_or(fallback, |k| s.nodes[k as usize].label());
            let class = k.and_then(|k| s.nodes[k as usize].class.as_deref()).filter(|c| *c != label).unwrap_or("");
            let picked = k.is_some() && k == self.ifc.pick;
            let content = row![text(label).size(12).color(if k.is_some() { p.text } else { p.text_muted }).wrapping(Wrapping::None), text(class).size(11).color(p.accent).wrapping(Wrapping::None)].spacing(6);
            let b = button(container(content).clip(true)).padding([0, 6]).width(280).style(move |_, st| if picked { p.ribbon_button(true, st) } else { p.row_button(st) });
            match k {
                Some(k) => b.on_press(Message::IfcPick(k)).into(),
                None => b.into(),
            }
        };
        let links = &self.ifc.links;
        let (first, last) = self.ifc_sight(IfcView::Relations, links.len());
        let mut rows = column![Space::new().height(first as f32 * LINE)];
        for &k in &links[first..last] {
            let link = &s.links[k as usize];
            let line = row![
                end(Some(link.from), ""),
                container(text(link.kind.ifc()).size(11).color(p.text_secondary).wrapping(Wrapping::None)).width(250).clip(true),
                container(text(link.name.as_str()).size(12).color(p.text).wrapping(Wrapping::None)).width(180).clip(true),
                end(link.to, link.target.as_str()),
            ]
            .spacing(6)
            .height(LINE)
            .align_y(Center);
            rows = rows.push(line);
        }
        rows = rows.push(Space::new().height((links.len() - last) as f32 * LINE));
        column![chips, rule::horizontal(1), self.ifc_scrollable(IfcView::Relations, rows, None)].into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model};

    /// A model of two walls (elements 10 and 20) with a level.
    fn app() -> App {
        let layer = Layer {
            name: "Walls".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0],
            normals: [0.0, 0.0, 1.0].repeat(4),
            indices: vec![0, 1, 2, 1, 3, 2],
            elements: vec![(10, 0, 3), (20, 3, 3)],
            props: vec![(10, r#"{"Name":"North wall","IfcType":"IfcWall"}"#.into()), (20, r#"{"Name":"South wall"}"#.into())],
            ..Default::default()
        };
        let mut model = Model::new("walls.rvt".into(), "Revit", vec![layer]);
        model.project.levels = vec![("00".into(), 0.0)];
        let mut app = App::default();
        app.set_model(Arc::new(model));
        app
    }

    fn structure(app: &App) -> Arc<Structure> {
        app.ifc.structure.clone().expect("the file taken apart")
    }

    fn shows(app: &App, wording: &str) -> bool {
        let mut ui = crate::app::test_ui(app, iced::Size::new(1400.0, 900.0));
        ui.find(wording).is_ok()
    }

    #[test]
    fn the_ifc_tab_shows_the_ifcx_save_as_writes() {
        let mut app = app();
        let _ = app.update(Message::OpenIfc);
        assert_eq!(app.doc_tabs(), [super::super::DocTab::Model, super::super::DocTab::Ifc]);
        assert_eq!(app.active_doc_tab(), 1);
        let s = structure(&app);
        assert_eq!(s.source, "walls.ifcx");
        // the two walls, the storey, the building and the project
        assert_eq!(s.entries.len(), 5, "{:?}", s.entries.iter().map(|e| &e.path).collect::<Vec<_>>());
        assert!(s.entries.iter().any(|e| e.class.as_deref() == Some("IfcWall") && e.name.as_deref() == Some("North wall")));
        assert_eq!(s.lines.first().map(|l| l.text.as_str()), Some("{"));
        // both walls are known by their node
        assert_eq!(app.ifc.node_of.len(), 2);
        // the 3D view's tab hides it, its own tab shows it again; closing it goes back to 3D
        let _ = app.update(Message::SelectTab(0));
        assert!(!app.ifc.shown);
        let _ = app.update(Message::SelectTab(1));
        assert!(app.ifc.shown);
        let _ = app.update(Message::CloseIfc);
        assert_eq!(app.doc_tabs(), [super::super::DocTab::Model]);
    }

    #[test]
    fn an_opened_ifcx_shows_its_own_text() {
        let dir = std::env::temp_dir().join(format!("o3v-ifc-tab-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("own.ifcx");
        let file = r#"{"header": {"ifcxVersion": "ifcx_alpha", "id": "own"}, "imports": [], "schemas": {}, "data": [
            {"path": "p", "children": {"Wall": "w"}, "attributes": {"bsi::ifc::class": {"code": "IfcProject"}}},
            {"path": "w", "children": {"Body": "w-body"}, "attributes": {"bsi::ifc::class": {"code": "IfcWall"}}},
            {"path": "w-body", "attributes": {"usd::usdgeom::mesh": {"points": [[0,0,0],[1,0,0],[0,1,0]], "faceVertexIndices": [0,1,2]}}}]}"#;
        std::fs::write(&path, file).unwrap();
        let model = crate::loaders::load(&path).unwrap();
        let mut app = App::default();
        app.set_model(Arc::new(model));
        let _ = app.update(Message::OpenIfc);
        let s = structure(&app);
        assert_eq!((s.source.as_str(), s.entries.len()), ("own.ifcx", 3));
        // the header as the file has it, not sorted
        assert_eq!(s.lines[2].text, "\"ifcxVersion\": \"ifcx_alpha\",");
        // the wall is the model's element; its body picks it too
        let wall = s.index["w"];
        assert_eq!(app.ifc.elements.len(), 1);
        let _ = app.update(Message::IfcPick(s.index["w-body"]));
        assert_eq!(app.ifc.elements.get(&wall), app.sel.primary().as_ref());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_node_picked_selects_its_elements_and_a_selected_element_picks_its_node() {
        let mut app = app();
        let _ = app.update(Message::OpenIfc);
        let s = structure(&app);
        // the storey selects both walls
        let storey = s.nodes.iter().position(|n| n.class.as_deref() == Some("IfcBuildingStorey")).unwrap() as u32;
        let _ = app.update(Message::IfcPick(storey));
        assert_eq!((app.ifc.pick, app.selection.len()), (Some(storey), 2));
        // one wall
        let north = s.nodes.iter().position(|n| n.name.as_deref() == Some("North wall")).unwrap() as u32;
        let _ = app.update(Message::IfcPick(north));
        assert_eq!(app.selected.map(|s| s.1), Some(Some(10)));
        // the other selected in the model: its node is picked, and the tree shows it
        app.sel.clear();
        app.sel.extend([Pickable::Element(20)]);
        app.selection_changed();
        let _ = app.update(Message::Fit);
        let south = s.nodes.iter().position(|n| n.name.as_deref() == Some("South wall")).unwrap() as u32;
        assert_eq!(app.ifc.pick, Some(south));
        assert!(app.ifc.rows.iter().any(|r| r.node == Some(south)));
    }

    #[test]
    fn a_search_finds_lines_nodes_entries_and_relations() {
        let mut app = app();
        let _ = app.update(Message::OpenIfc);
        let s = structure(&app);
        let south = s.nodes.iter().position(|n| n.name.as_deref() == Some("South wall")).unwrap() as u32;
        let _ = app.update(Message::IfcQuery("SOUTH".into()));
        // the text: its name and its name as the storey's child, the first one jumped to
        assert_eq!(app.ifc.hits.len(), 2);
        assert!(app.ifc.hits.iter().all(|&l| s.lines[l as usize].text.to_lowercase().contains("south")));
        assert_eq!(app.ifc.hit, 0);
        let _ = app.update(Message::IfcNext(true));
        assert_eq!(app.ifc.hit, 1);
        let _ = app.update(Message::IfcNext(true));
        assert_eq!(app.ifc.hit, 0, "past the last one it starts again");
        let _ = app.update(Message::IfcNext(false));
        assert_eq!(app.ifc.hit, 1);
        // the nodes: the wall, with the rows above it only
        assert_eq!(app.ifc.matches, HashSet::from([south]));
        let key = s.keys[&south].clone();
        assert!(app.ifc.rows.iter().any(|r| r.node == Some(south)));
        assert!(app.ifc.rows.iter().all(|r| r.node == Some(south) || key.starts_with(&r.key)), "{:?}", app.ifc.rows);
        // the table: its entry; the relations: the storey holding it
        assert_eq!(app.ifc.table.len(), 1);
        assert_eq!(app.ifc.links.len(), 1);
        assert_eq!(s.links[app.ifc.links[0] as usize].to, Some(south));
        // the field and the count are drawn
        assert!(shows(&app, "2 / 2"));
        // an empty search shows everything again, the tree as it was opened
        let _ = app.update(Message::IfcQuery(String::new()));
        assert_eq!((app.ifc.hits.len(), app.ifc.table.len(), app.ifc.links.len()), (0, s.entries.len(), s.links.len()));
        assert!(app.ifc.rows.len() > 2);
    }

    #[test]
    fn the_four_views_are_drawn() {
        let mut app = app();
        let _ = app.update(Message::OpenIfc);
        // Save as writes the members of the file sorted: `data` first
        for wording in ["walls.ifcx", "Text", "Nodes", "Table", "Relations", "  \"data\": ["] {
            assert!(shows(&app, wording), "{wording}");
        }
        let _ = app.update(Message::IfcView(IfcView::Nodes));
        for wording in ["walls", "IfcProject", "Building", "00", "IfcBuildingStorey"] {
            assert!(shows(&app, wording), "{wording}");
        }
        let _ = app.update(Message::IfcView(IfcView::Table));
        for wording in ["Path", "Class", "Children", "IfcWall", "North wall"] {
            assert!(shows(&app, wording), "{wording}");
        }
        let _ = app.update(Message::IfcView(IfcView::Relations));
        // project - building - storey, the storey holding the two walls
        for wording in ["All 4", "IfcRelAggregates 2", "IfcRelContainedInSpatialStructure 2", "North wall"] {
            assert!(shows(&app, wording), "{wording}");
        }
        let _ = app.update(Message::IfcRelation(Some(Relation::Contains)));
        assert_eq!(app.ifc.links.len(), 2);
    }
}
