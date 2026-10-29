//! Schedules (Revit): the explorer group, the document tabs and the table under the viewport.
//! A click on a row selects its elements in the model. The table draws only the rows in sight,
//! so a schedule of a thousand walls keeps the viewport smooth.

use super::{group_header, App, Message, FONT, FONT_BOLD};
use crate::model::Schedule;
use crate::theme::Palette;
use iced::widget::operation::AbsoluteOffset;
use iced::widget::scrollable::{Direction, RelativeOffset, Scrollbar, Viewport};
use iced::widget::text::Wrapping;
use iced::widget::{button, column, container, operation, row, rule, scrollable, text, Column, Space};
use iced::{Center, Element, Fill, FillPortion, Padding, Task};
use openaec_ui::Language;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// The document tab of the 3D view.
pub const VIEW_3D: &str = "3d";
/// Height of a table row (logical pixels).
const ROW: f32 = 22.0;
/// The scrollable of the table rows, to show a schedule from its top, and that of the headings.
const TABLE: &str = "schedule-rows";
const HEADER: &str = "schedule-headings";

/// The document tab of a schedule.
pub fn tab_id(schedule: usize) -> String {
    format!("schedule:{schedule}")
}

/// The schedule of a document tab.
pub fn tab_schedule(id: &str) -> Option<usize> {
    id.strip_prefix("schedule:")?.parse().ok()
}

/// Runs the schedules a loader left (see [`crate::model::Later`]) on a worker thread.
#[cfg(not(target_arch = "wasm32"))]
fn later_task(id: u64, work: Box<dyn FnOnce() -> Vec<Schedule> + Send>) -> Task<Message> {
    Task::perform(
        async move {
            let (tx, rx) = iced::futures::channel::oneshot::channel();
            std::thread::spawn(move || {
                let _ = tx.send(work());
            });
            rx.await.unwrap_or_default()
        },
        move |schedules| Message::Schedules(id, schedules),
    )
}

/// The browser has no threads: the schedules are computed in the task.
#[cfg(target_arch = "wasm32")]
fn later_task(id: u64, work: Box<dyn FnOnce() -> Vec<Schedule> + Send>) -> Task<Message> {
    Task::perform(async move { work() }, move |schedules| Message::Schedules(id, schedules))
}

impl App {
    /// Starts computing the schedules the loader left for after the model shows.
    pub(super) fn compute_schedules(&mut self) -> Task<Message> {
        let Some(model) = &self.model else { return Task::none() };
        let Some(work) = model.project.schedules.take() else { return Task::none() };
        self.schedules_pending = true;
        later_task(model.id, work)
    }

    /// Opens a schedule in its own tab, or shows its tab when it is open.
    pub(super) fn open_schedule(&mut self, i: usize) -> Task<Message> {
        if !self.open_schedules.contains(&i) {
            self.open_schedules.push(i);
        }
        self.show_schedule(Some(i))
    }

    /// Shows a schedule's tab, from its top (`None`: the 3D view alone).
    pub(super) fn show_schedule(&mut self, i: Option<usize>) -> Task<Message> {
        if self.active_schedule == i {
            return Task::none();
        }
        self.active_schedule = i;
        self.schedule_scroll = (0.0, 0.0);
        Task::batch([operation::snap_to(TABLE, RelativeOffset::START), operation::snap_to(HEADER, RelativeOffset::START)])

    }

    /// Closes a schedule's tab; when it was shown, the tab before it (or the 3D view) shows.
    pub(super) fn close_schedule(&mut self, i: usize) -> Task<Message> {
        let Some(k) = self.open_schedules.iter().position(|&s| s == i) else { return Task::none() };
        self.open_schedules.remove(k);
        if self.active_schedule != Some(i) {
            return Task::none();
        }
        let next = k.checked_sub(1).or((!self.open_schedules.is_empty()).then_some(0)).map(|k| self.open_schedules[k]);
        self.show_schedule(next)
    }

    /// Selects the drawn elements of a schedule row; the first of them shows its properties.
    pub(super) fn select_row(&mut self, schedule: usize, row: usize) {
        let Some(model) = self.model.clone() else { return };
        let Some(r) = self.schedules.get(schedule).and_then(|s| s.rows.get(row)) else { return };
        let ids: HashSet<u64> = r.elements.iter().copied().collect();
        let mut layer_of = HashMap::new();
        self.selection.clear();
        for (l, layer) in model.layers.iter().enumerate() {
            for &(id, first, count) in layer.elements.iter().filter(|e| ids.contains(&e.0)) {
                self.selection.push((l, first, count));
                layer_of.entry(id).or_insert(l);
            }
        }
        self.selected = r.elements.iter().find_map(|id| Some((*layer_of.get(id)?, Some(*id))));
    }

    /// The explorer group of the schedules, by name (Revit's "Schedules/Quantities").
    pub(super) fn schedules_group<'a>(&self, p: Palette, l: Language, query: &str, mut body: Column<'a, Message>) -> Column<'a, Message> {
        let schedules = &self.schedules;
        if self.schedules_pending {
            let tag = l.text("rekent…", "computing…");
            return body.push(group_header(p, l.text("Staten", "Schedules"), Some(tag), 0, false, Message::ToggleExplorer(3)));
        }
        if schedules.is_empty() {
            return body;
        }
        let shown: Vec<usize> = (0..schedules.len()).filter(|&i| query.is_empty() || schedules[i].name.to_lowercase().contains(query)).collect();
        let open = self.explorer_open[3] || !query.is_empty();
        body = body.push(group_header(p, l.text("Staten", "Schedules"), Some("beta"), shown.len(), open, Message::ToggleExplorer(3)));
        if !open {
            return body;
        }
        for i in shown {
            let active = self.active_schedule == Some(i);
            // schedules without computed rows (material takeoffs, key schedules) are muted
            let color = if active { p.accent } else if schedules[i].computed { p.text } else { p.text_muted };
            let item = button(text(schedules[i].name.clone()).size(12).color(color).wrapping(Wrapping::WordOrGlyph).width(Fill))
                .padding([3, 6])
                .width(Fill)
                .style(move |_, s| if active { p.ribbon_button(true, s) } else { p.row_button(s) })
                .on_press(Message::OpenSchedule(i));
            body = body.push(container(item).padding(Padding { left: 14.0, ..Padding::ZERO }).width(Fill));
        }
        body
    }

    /// The workspace: the viewport, with the table of the shown schedule under it.
    pub fn workspace_view(&self, p: Palette, l: Language) -> Element<'_, Message> {
        let viewport = self.viewport_view(p, l);
        let Some((i, s)) = self.active_schedule.and_then(|i| Some((i, self.schedules.get(i)?))) else {

            return viewport;
        };
        column![
            container(viewport).height(FillPortion(3)),
            rule::horizontal(1),
            container(self.schedule_table(p, l, i, s)).height(FillPortion(2)).style(move |_| p.panel()),
        ]
        .into()
    }

    /// A schedule as a table: its title and row count, the headings, then the rows in sight.
    /// Columns are as wide as their longest value (up to a limit); a row with drawn elements
    /// selects them, the row of the selected element is marked.
    fn schedule_table<'a>(&'a self, p: Palette, l: Language, index: usize, s: &'a Schedule) -> Element<'a, Message> {
        let Some(model) = &self.model else { return Space::new().into() };
        let widths: Vec<f32> = (0..s.columns.len())
            .map(|c| {
                let longest = s.rows.iter().take(1000).filter_map(|r| r.cells.get(c)?.as_deref()).map(|v| v.chars().count());
                let chars = longest.chain([s.columns[c].chars().count()]).max().unwrap_or(1);
                (chars as f32 * 7.0 + 14.0).clamp(48.0, 320.0)
            })
            .collect();
        let width: f32 = widths.iter().sum();
        let cell = |t: iced::widget::Text<'a>, w: f32| container(t.wrapping(Wrapping::None)).width(w).padding([0, 6]).clip(true);

        let header = row(s.columns.iter().zip(&widths).map(|(h, &w)| cell(text(h.as_str()).size(11).font(FONT_BOLD).color(p.text_secondary), w).into()))
            .height(ROW + 4.0)
            .align_y(Center);

        // the rows in sight, with space for those above and below (the height is known after
        // the first scroll; until then, enough rows for a tall window)
        let (offset, height) = self.schedule_scroll;
        let first = ((offset / ROW).floor().max(0.0) as usize).min(s.rows.len());
        let last = (first + (height.max(1200.0) / ROW).ceil() as usize + 2).min(s.rows.len());
        let selected = self.selected.and_then(|(_, id)| id);
        let mut rows = column![Space::new().height(first as f32 * ROW)];
        for (k, r) in s.rows[first..last].iter().enumerate() {
            let active = selected.is_some_and(|id| r.elements.contains(&id));
            let font = if r.total { FONT_BOLD } else { FONT };
            let cells = row(r.cells.iter().zip(&widths).map(|(v, &w)| {
                let t = match v {
                    Some(v) => text(v.as_str()).color(if active { p.accent } else { p.text }),
                    None => text("?").color(p.text_muted),
                };
                cell(t.size(12).font(font), w).into()
            }))
            .height(ROW)
            .align_y(Center);
            let drawn = !r.total && r.elements.iter().any(|id| model.props.contains_key(id));
            rows = rows.push(if drawn {
                Element::from(
                    button(cells)
                        .padding(0)
                        .style(move |_, st| if active { p.ribbon_button(true, st) } else { p.row_button(st) })
                        .on_press(Message::ScheduleRow(index, first + k)),
                )
            } else {
                cells.into()
            });
        }
        rows = rows.push(Space::new().height((s.rows.len() - last) as f32 * ROW));

        let count = s.rows.iter().filter(|r| !r.total).count();
        let info = if s.computed {
            format!("{count} {}", l.text("rijen", "rows"))
        } else {
            l.text("rijen van materiaal- en sleutelstaten worden nog niet berekend", "rows of material takeoffs and key schedules are not computed yet")
                .to_string()
        };
        let close = button(text("×").size(14).color(p.text_secondary))
            .padding([0, 6])
            .style(move |_, st| p.row_button(st))
            .on_press(Message::CloseSchedule(index));
        let bar = row![
            text(s.title.as_str()).size(12).font(FONT_BOLD).color(p.text),
            text(info).size(11).color(p.text_muted),
            Space::new().width(Fill),
            close,
        ]
        .spacing(10)
        .align_y(Center)
        .padding([4, 8]);

        // the headings stay in place while the rows scroll, and follow them sideways (see
        // `scrolled`); both scrollbars sit at the edges of the pane
        let both = Direction::Both { vertical: Scrollbar::new(), horizontal: Scrollbar::new() };
        column![
            bar,
            scrollable(header.width(width)).id(HEADER).direction(Direction::Horizontal(Scrollbar::hidden())).width(Fill),
            rule::horizontal(1),
            scrollable(rows.width(width)).id(TABLE).direction(both).on_scroll(Message::ScheduleScroll).width(Fill).height(Fill),
        ]
        .into()
    }

    /// The rows scrolled: remembers which are in sight and moves the headings along.
    pub(super) fn scrolled(&mut self, v: Viewport) -> Task<Message> {
        self.schedule_scroll = (v.absolute_offset().y, v.bounds().height);
        operation::scroll_to(HEADER, AbsoluteOffset { x: Some(v.absolute_offset().x), y: None })
    }
}

// ---- remote control ----

/// Element ids listed per row at most (a group row can hold a thousand walls).
const REMOTE_IDS: usize = 100;

impl App {
    /// Remote `list_schedules`: every schedule with its columns and row count.
    pub(crate) fn schedules_json(&self) -> Result<Value, String> {
        self.model.as_ref().ok_or("no model is open: call open_file first")?;
        let list: Vec<Value> = self
            .schedules
            .iter()
            .enumerate()
            .map(|(i, s)| {
                json!({
                    "index": i,
                    "name": s.name,
                    "columns": s.columns,
                    "rows": s.rows.iter().filter(|r| !r.total).count(),
                    "computed": s.computed,
                    "shown": self.active_schedule == Some(i),
                })
            })
            .collect();
        Ok(json!({ "computing": self.schedules_pending, "count": list.len(), "schedules": list }))
    }

    /// Remote `get_schedule`: a schedule's rows (`offset`, `limit`); with `show`, its table opens
    /// under the viewport.
    pub(crate) fn remote_schedule(&mut self, a: &Value) -> (Result<Value, String>, Task<Message>) {
        let i = match self.find_schedule(a.get("schedule")) {
            Ok(i) => i,
            Err(e) => return (Err(e), Task::none()),
        };
        let s = &self.schedules[i];
        let offset = a.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let limit = a.get("limit").and_then(Value::as_u64).unwrap_or(100) as usize;
        let rows: Vec<Value> = s
            .rows
            .iter()
            .skip(offset)
            .take(limit)
            .map(|r| {
                let mut row = json!({ "cells": r.cells, "element_count": r.elements.len() });
                if r.total {
                    row["total"] = json!(true);
                } else {
                    row["elements"] = json!(r.elements.iter().take(REMOTE_IDS).collect::<Vec<_>>());
                }
                row
            })
            .collect();
        let answer = json!({
            "index": i,
            "name": s.name,
            "title": s.title,
            "columns": s.columns,
            "computed": s.computed,
            "row_count": s.rows.len(),
            "offset": offset,
            "rows": rows,
            "note": "cells are as Revit shows them; null means the reader could not work the value out",
        });
        let show = a.get("show").and_then(Value::as_bool).unwrap_or(false);
        (Ok(answer), if show { self.open_schedule(i) } else { Task::none() })
    }

    /// A schedule by index or name (exact, else the first containing it, ignoring case).
    fn find_schedule(&self, spec: Option<&Value>) -> Result<usize, String> {
        self.model.as_ref().ok_or("no model is open: call open_file first")?;
        if self.schedules_pending {
            return Err("the schedules are still being computed: try again in a few seconds".into());
        }
        let spec = spec.ok_or("schedule (index or name) is required")?;
        if let Some(i) = spec.as_u64() {
            return (i < self.schedules.len() as u64).then_some(i as usize).ok_or(format!("no schedule {i}"));
        }
        let name = spec.as_str().ok_or("schedule must be an index or a name")?.to_lowercase();
        let names: Vec<String> = self.schedules.iter().map(|s| s.name.to_lowercase()).collect();
        names
            .iter()
            .position(|n| *n == name)
            .or_else(|| names.iter().position(|n| n.contains(&name)))
            .ok_or(format!("no schedule named '{name}'"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Layer, Model, ScheduleRow};
    use std::sync::Arc;

    #[test]
    fn tab_ids() {
        assert_eq!(tab_schedule(&tab_id(12)), Some(12));
        assert_eq!(tab_schedule(VIEW_3D), None);
    }

    /// Two drawn elements (7 and 8) and an element without geometry (9).
    fn app_with_schedule() -> App {
        let layer = Layer {
            name: "Doors".into(),
            positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0],
            normals: [0.0, 0.0, 1.0].repeat(4),
            indices: vec![0, 1, 2, 1, 3, 2],
            elements: vec![(7, 0, 3), (8, 3, 3)],
            ..Default::default()
        };
        let mut app = App::default();
        app.set_model(Arc::new(Model::new("doors.rvt".into(), "Revit", vec![layer])));
        let row = |cells: &[&str], elements: Vec<u64>| ScheduleRow { cells: cells.iter().map(|c| Some(c.to_string())).collect(), elements, total: false };
        app.schedules = vec![Schedule {
            name: "Door Schedule".into(),
            columns: vec!["Mark".into()],
            rows: vec![row(&["D1"], vec![9, 8]), row(&["D2, D3"], vec![7, 8])],
            computed: true,
            ..Default::default()
        }];
        app
    }

    #[test]
    fn a_row_selects_its_drawn_elements() {
        let mut app = app_with_schedule();
        app.select_row(0, 1);
        assert_eq!(app.selection.len(), 2);
        assert_eq!(app.selected.map(|s| s.1), Some(Some(7)));
        // the first drawn element of the row shows its properties
        app.select_row(0, 0);
        assert_eq!(app.selected.map(|s| s.1), Some(Some(8)));
        assert_eq!(app.selection.len(), 1);
    }

    #[test]
    fn closing_the_shown_tab_shows_the_one_before() {
        let mut app = app_with_schedule();
        app.schedules.push(app.schedules[0].clone());
        let _ = app.open_schedule(0);
        let _ = app.open_schedule(1);
        assert_eq!(app.active_schedule, Some(1));
        let _ = app.close_schedule(1);
        assert_eq!((app.open_schedules.clone(), app.active_schedule), (vec![0], Some(0)));
        let _ = app.close_schedule(0);
        assert_eq!(app.active_schedule, None);
        // a new model closes the tabs
        let _ = app.open_schedule(0);
        let model = app.model.clone().unwrap();
        app.set_model(model);
        assert!(app.open_schedules.is_empty() && app.schedules.is_empty());
    }
}
