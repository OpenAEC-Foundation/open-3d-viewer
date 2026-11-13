//! The views of the drawings cut ahead, on worker threads: cutting a plan, section or elevation
//! through a large model takes seconds, and a drawing holds several. Once the drawings of a file
//! are read, every view placed on one is cut in the background, those of the drawing being opened
//! first; a drawing then shows its views as they come in, and one opened later has them already.

use super::{App, Message};
use crate::document::PlanKind;
use crate::drawing::{ceiling_at, plan_at, section_at, structural_at, Cut, Shown};
use crate::explode::ExplodeIndex;
use crate::model::Model;
use iced::Task;
use std::sync::Arc;

/// What a view cuts through.
#[derive(Debug, Clone, Copy)]
pub(super) enum CutKind {
    Section { from: [f64; 2], to: [f64; 2], depth: f64 },
    Plan { kind: PlanKind, low: f64, high: f64 },
}

/// All a cut of a view needs, taken from the app so that it can be made on another thread.
pub(super) struct CutJob {
    pub model: Arc<Model>,
    pub layers: Vec<bool>,
    /// The units (see [`ExplodeIndex`]) the view leaves out, and those it draws in halftone.
    pub units: Vec<bool>,
    pub halftone: Vec<bool>,
    pub index: Option<Arc<ExplodeIndex>>,
    pub kind: CutKind,
    pub scale: f64,
}

impl CutJob {
    /// The cut, with its cut faces hatched.
    pub(super) fn run(self) -> Cut {
        let CutJob { model, layers, units, halftone, index, kind, scale } = self;
        let index = index.filter(|_| units.iter().chain(&halftone).any(|u| *u));
        let hidden = |l: usize, v: u32| index.as_ref().is_some_and(|ix| units.get(ix.unit(l, v)).copied().unwrap_or(false));
        let faint = |l: usize, v: u32| index.as_ref().is_some_and(|ix| halftone.get(ix.unit(l, v)).copied().unwrap_or(false));
        // the pieces joined elements of one material make: the seams within one are not drawn
        let pieces = crate::hatching::pieces(&model, bim_library::DrawingStandard::Nen47);
        let shown = Shown { layers: &layers, hidden: &hidden, faint: halftone.iter().any(|h| *h).then_some(&faint as &dyn Fn(usize, u32) -> bool), pieces: (!pieces.is_empty()).then_some(&pieces) };
        let mut cut = match kind {
            CutKind::Section { from, to, depth } => section_at(&model, shown, from, to, depth),
            CutKind::Plan { kind, low, high } if kind.looks_up() => ceiling_at(&model, shown, low as f32, high as f32),
            // a structural plan looks through the floor: what lies under it is drawn dashed
            CutKind::Plan { kind: PlanKind::Structural, low, high } => structural_at(&model, shown, low as f32, high as f32),
            CutKind::Plan { low, high, .. } => plan_at(&model, shown, low as f32, high as f32),
        };
        // the cut faces in the hatch of what they are made of
        crate::hatching::hatch(&mut cut, &model, bim_library::DrawingStandard::Nen47, scale);
        cut
    }
}

/// How many views are cut at a time: half the cores, so the window stays quick.
fn workers() -> usize {
    std::thread::available_parallelism().map_or(2, |n| n.get() / 2).clamp(1, 8)
}

impl App {
    /// Queues the views of every drawing of the file to be cut ahead, in the order of the
    /// drawings, and starts cutting.
    pub(super) fn cut_ahead(&mut self) -> Task<Message> {
        if !self.background_cuts {
            return Task::none();
        }
        let views: Vec<String> = self.sheets().iter().flat_map(|s| s.viewports.iter().filter_map(|v| v.drawing.clone())).collect();
        for view in views {
            if !self.cuts.contains_key(&view) && !self.cut_queue.contains(&view) {
                self.cut_queue.push_back(view);
            }
        }
        self.cut_next()
    }

    /// Puts `views` at the front of the queue (those of the drawing being opened) and goes on
    /// cutting.
    pub(super) fn cut_first(&mut self, views: Vec<String>) -> Task<Message> {
        for view in views.into_iter().rev() {
            self.cut_queue.retain(|queued| *queued != view);
            if !self.cuts.contains_key(&view) && !self.cutting.contains(&view) {
                self.cut_queue.push_front(view);
            }
        }
        self.cut_next()
    }

    /// Starts cutting the next views of the queue, as many as there are workers free.
    pub(super) fn cut_next(&mut self) -> Task<Message> {
        let mut tasks = Vec::new();
        while self.cutting.len() < workers() {
            let Some(view) = self.cut_queue.pop_front() else { break };
            if self.cuts.contains_key(&view) || self.cutting.contains(&view) {
                continue;
            }
            let Some(job) = self.cut_job(&view) else { continue };
            self.cutting.insert(view.clone());
            let epoch = self.cut_epoch;
            tasks.push(Task::perform(
                async move {
                    let (tx, rx) = iced::futures::channel::oneshot::channel();
                    std::thread::spawn(move || {
                        let _ = tx.send(job.run());
                    });
                    rx.await.ok().map(Arc::new)
                },
                move |cut| Message::CutAhead(epoch, view.clone(), cut),
            ));
        }
        Task::batch(tasks)
    }

    /// Takes a view cut ahead: kept when nothing changed what views show since it was started.
    pub(super) fn take_cut(&mut self, epoch: u64, view: String, cut: Option<Arc<Cut>>) -> Task<Message> {
        self.cutting.remove(&view);
        match cut {
            Some(cut) if epoch == self.cut_epoch => {
                self.cuts.entry(view).or_insert(cut);
            }
            // cut for what the views showed before: cut again
            Some(_) => self.cut_queue.push_back(view),
            None => {}
        }
        self.cut_next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A view cut ahead is kept; one that was started before what views show changed is cut again.
    #[test]
    fn a_cut_started_before_a_change_is_not_kept() {
        let mut app = App::default();
        app.cutting.insert("plan".into());
        let _ = app.take_cut(0, "plan".into(), Some(Arc::new(Cut::default())));
        assert!(app.cuts.contains_key("plan") && app.cutting.is_empty());
        app.cuts.clear();
        app.cut_epoch += 1;
        let _ = app.take_cut(0, "plan".into(), Some(Arc::new(Cut::default())));
        assert!(!app.cuts.contains_key("plan"));
    }
}
